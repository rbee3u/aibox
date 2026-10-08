import styles from "@/features/requests/detail/BodyViewer.module.css";
import bodySharedStyles from "@/features/requests/detail/bodyShared.module.css";
import { SseEventList } from "@/features/requests/detail/SseEventList";
import { toggleBodyViewSet } from "@/features/requests/detail/bodyViewMemory";
import { Check, Clipboard, Download, LoaderCircle } from "lucide-react";
import { useMemo } from "react";
import type { ReactNode } from "react";
import {
  bodyComplete,
  bodyHeaders,
  bodyMediaType,
  contentCoding,
  decodeUtf8,
  isEncodedContentCoding,
  isJsonMediaType,
  isSseResponse,
  parseJson,
  parseSse,
  shouldDeferPretty,
  type ContentCoding,
} from "@/features/requests/detail/bodyPresentation";
import type { BodyViewMemory } from "@/features/requests/detail/bodyViewMemory";
import type { BodyKind, EventTimingIndex, RequestDetail } from "@/api/requests";
import type { BodyLoadStatus, DecodedBodyState } from "@/features/requests/viewTypes";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { concatChunks, formatByteSize, hex } from "@/shared/lib/format";
import { JsonTree } from "@/features/requests/detail/JsonTree";

import { SegmentedControl } from "@/shared/ui/SegmentedControl";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { iconSize } from "@/shared/icons/iconSizes";
import { IconButton } from "@/shared/ui/IconButton";

interface BodyViewerProps {
  kind: BodyKind;
  detail: RequestDetail;
  bodyChunks: Uint8Array[];
  bodyStatus: BodyLoadStatus;
  decoded: DecodedBodyState;
  timings: EventTimingIndex | null;
  loadingBody: boolean;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
  onDownload: () => void;
}

export function BodyViewer({
  kind,
  detail,
  bodyChunks,
  bodyStatus,
  decoded,
  timings,
  loadingBody,
  memory,
  onMemoryChange,
  onDownload,
}: BodyViewerProps) {
  const [bodyCopied, copyBodyText] = useClipboardFeedback();
  const headers = bodyHeaders(detail, kind);
  const coding = contentCoding(headers);
  const original = useMemo(() => concatChunks(bodyChunks), [bodyChunks]);
  const complete = bodyComplete(detail, kind);
  const sourceBytes =
    coding.kind === "identity"
      ? original
      : isEncodedContentCoding(coding.kind)
        ? decoded.bytes
        : null;
  const decodedText = useMemo(
    () => (sourceBytes ? decodeUtf8(sourceBytes, complete) : null),
    [complete, sourceBytes],
  );
  const mediaType = bodyMediaType(headers);
  const declaredJson = isJsonMediaType(mediaType);
  const sse = kind === "response" && isSseResponse(detail);
  const large = shouldDeferPretty(sourceBytes?.length ?? 0);
  const canParse = decodedText?.ok === true && (!large || memory.renderLarge);
  const parsedJson = useMemo(
    () => (canParse && !sse && complete ? parseJson(decodedText.text) : null),
    [canParse, complete, decodedText, sse],
  );
  const parsedEvents = useMemo(
    () => (canParse && sse ? parseSse(decodedText.text) : null),
    [canParse, decodedText, sse],
  );
  const jsonPretty = parsedJson?.ok === true;
  const pendingDecode =
    isEncodedContentCoding(coding.kind) && decoded.bytes === null && decoded.error === null;
  const pendingPretty =
    coding.kind !== "unsupported" && ((declaredJson && !complete) || pendingDecode);
  const prettyAvailable = sse ? parsedEvents !== null : jsonPretty;
  const resolvedMode =
    memory.mode === "pretty" && (prettyAvailable || pendingPretty) ? "pretty" : "source";
  const canRenderLarge = large && !memory.renderLarge && decodedText?.ok === true;
  const originalSize = kind === "request" ? detail.request_body_bytes : detail.response_body_bytes;
  const decodedSize = isEncodedContentCoding(coding.kind) ? sourceBytes?.length : undefined;

  function copyBody() {
    if (decodedText?.ok) void copyBodyText(decodedText.text, true);
  }

  const updateSet = (key: "expandedNodes" | "expandedStrings", value: string): BodyViewMemory => ({
    ...memory,
    [key]: toggleBodyViewSet(memory[key], value),
  });

  let bodyContent: ReactNode;
  if (bodyStatus === "error") {
    bodyContent = <BodyState>Original Body unavailable.</BodyState>;
  } else if (bodyStatus === "idle") {
    bodyContent = <BodyState loading>Loading Body…</BodyState>;
  } else if (resolvedMode === "pretty" && pendingPretty) {
    const message = isEncodedContentCoding(coding.kind)
      ? complete
        ? `Decoding ${coding.kind} Body…`
        : `Waiting for the complete ${coding.kind} Body before decoding…`
      : "Waiting for the complete JSON Body…";
    bodyContent = <BodyState loading>{message}</BodyState>;
  } else if (resolvedMode === "pretty" && parsedEvents) {
    bodyContent = (
      <SseEventList
        requestId={detail.request.id}
        events={parsedEvents.events}
        partial={parsedEvents.hasPartialTail}
        active={detail.state === "active"}
        observedAt={detail.request.started_at}
        timings={timings}
        memory={memory}
        onMemoryChange={onMemoryChange}
      />
    );
  } else if (resolvedMode === "pretty" && parsedJson?.ok) {
    bodyContent = (
      <JsonTree
        value={parsedJson.value}
        expanded={memory.expandedNodes}
        expandedStrings={memory.expandedStrings}
        onToggle={(path) => onMemoryChange(updateSet("expandedNodes", path))}
        onToggleString={(path) => onMemoryChange(updateSet("expandedStrings", path))}
      />
    );
  } else {
    bodyContent = (
      <SourceView
        original={original}
        decodedText={decodedText}
        coding={coding.kind}
        message={sourceMessage({
          coding,
          decoded,
          invalidUtf8: decodedText?.ok === false,
          declaredJson,
          complete,
          large: large && !memory.renderLarge,
          parseError: parsedJson && !parsedJson.ok ? parsedJson.message : null,
          mediaType,
          prettyAvailable,
        })}
        onRenderLarge={
          canRenderLarge
            ? () => onMemoryChange({ ...memory, renderLarge: true, mode: "pretty" })
            : undefined
        }
      />
    );
  }

  return (
    <>
      <div className={styles.sectionTitle}>
        <h2>
          Body <span>· {formatByteSize(originalSize)}</span>
          {decodedSize !== undefined && decodedSize !== originalSize && (
            <span>· {formatByteSize(decodedSize)} decoded</span>
          )}
        </h2>
        <div className={styles.bodyActions}>
          <SegmentedControl variant="filled" role="group" aria-label={`${kind} body view`}>
            <button
              type="button"
              aria-pressed={resolvedMode === "pretty"}
              disabled={!prettyAvailable && !pendingPretty}
              onClick={() => onMemoryChange({ ...memory, mode: "pretty" })}
            >
              Pretty
            </button>
            <button
              type="button"
              aria-pressed={resolvedMode === "source"}
              onClick={() => onMemoryChange({ ...memory, mode: "source" })}
            >
              Source
            </button>
          </SegmentedControl>
          {loadingBody && (
            <LoaderCircle
              className={`${styles.loading} spin`}
              size={iconSize.xs}
              aria-label="Loading body"
            />
          )}
          <IconButton
            size="sm"
            onClick={copyBody}
            disabled={!decodedText?.ok}
            label={bodyCopied ? "Body Source copied" : "Copy decoded Body Source"}
          >
            {bodyCopied ? (
              <Check size={iconSize.xs} aria-hidden="true" />
            ) : (
              <Clipboard size={iconSize.xs} aria-hidden="true" />
            )}
          </IconButton>
          <IconButton size="sm" onClick={onDownload} label="Download original body">
            <Download size={iconSize.xs} aria-hidden="true" />
          </IconButton>
        </div>
      </div>
      <p className={styles.sensitiveContext}>
        Raw Body data may contain sensitive values and is displayed without redaction.
      </p>
      {bodyContent}
    </>
  );
}

function SourceView({
  original,
  decodedText,
  coding,
  message,
  onRenderLarge,
}: {
  original: Uint8Array;
  decodedText: ReturnType<typeof decodeUtf8> | null;
  coding: ContentCoding["kind"];
  message: string | null;
  onRenderLarge?: () => void;
}) {
  const hexSource = decodedText?.ok === false ? decodedText.hex : hex(original);
  const source =
    decodedText?.ok === true
      ? decodedText.text || "(empty body)"
      : `hex: ${hexSource || "(empty body)"}`;
  return (
    <div className={bodySharedStyles.sourceWrap}>
      {(message || onRenderLarge) && (
        <AlertBanner
          className={bodySharedStyles.bodyNotice}
          tone="warning"
          action={
            onRenderLarge ? (
              <button type="button" onClick={onRenderLarge}>
                Render Pretty
              </button>
            ) : undefined
          }
        >
          {message}
        </AlertBanner>
      )}
      {coding !== "identity" && !decodedText && (
        <div className={styles.encodedLabel}>Encoded original bytes</div>
      )}
      <pre className={styles.body}>{source}</pre>
    </div>
  );
}

function BodyState({ children, loading = false }: { children: ReactNode; loading?: boolean }) {
  return (
    <div className={styles.bodyState} role="status">
      {loading && (
        <LoaderCircle className={`${styles.loading} spin`} size={iconSize.sm} aria-hidden="true" />
      )}
      {children}
    </div>
  );
}

function sourceMessage({
  coding,
  decoded,
  invalidUtf8,
  declaredJson,
  complete,
  large,
  parseError,
  mediaType,
  prettyAvailable,
}: {
  coding: ContentCoding;
  decoded: DecodedBodyState;
  invalidUtf8: boolean;
  declaredJson: boolean;
  complete: boolean;
  large: boolean;
  parseError: string | null;
  mediaType: string | null;
  prettyAvailable: boolean;
}): string | null {
  if (prettyAvailable) return null;
  if (coding.kind === "unsupported") return coding.message;
  if (decoded.error) return decoded.error;
  if (isEncodedContentCoding(coding.kind) && decoded.bytes === null) {
    return complete
      ? `Decoding ${coding.kind} Body.`
      : `Waiting for the complete ${coding.kind} Body before decoding.`;
  }
  if (invalidUtf8) return "Decoded Body is not valid UTF-8; showing decoded bytes as hex.";
  if (large)
    return "Decoded Body is larger than 5 MiB; Source is shown to avoid expensive rendering.";
  if (declaredJson && !complete) return "Pretty will be available when the JSON Body is complete.";
  if (parseError && declaredJson) return `Pretty unavailable: ${parseError}`;
  if (mediaType) return `No Pretty renderer for ${mediaType}; showing Source.`;
  if (parseError) return "Body is not JSON; showing Source.";
  return null;
}
