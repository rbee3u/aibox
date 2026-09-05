import { Check, ChevronDown, ChevronRight, Clipboard, Download, LoaderCircle } from "lucide-react";
import { useLayoutEffect, useMemo, useRef } from "react";
import type { ReactNode } from "react";
import {
  bodyComplete,
  bodyHeaders,
  bodyMediaType,
  contentCoding,
  decodeUtf8,
  eventAbsoluteTime,
  eventRelativeTime,
  groupSseEventsWithoutPreview,
  isEncodedContentCoding,
  isJsonMediaType,
  isSseResponse,
  parseJson,
  parseSse,
  presentSseEvent,
  shouldDeferPretty,
  shouldPinSseListToBottom,
  sseEventRunLabel,
  stringifyJson,
  type ContentCoding,
  type ParsedSseEvent,
  type PresentedSseEvent,
  type SseListEntry,
} from "@/features/requests/detail/bodyPresentation";
import type { BodyViewMemory } from "@/features/requests/detail/bodyViewMemory";
import type { BodyKind, EventTimingEntry, EventTimingIndex, RequestDetail } from "@/api/requests";
import type { BodyLoadStatus, DecodedBodyState } from "@/features/requests/viewTypes";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { concatChunks, formatByteSize, hex } from "@/shared/lib/format";
import { JsonTree } from "@/features/requests/detail/JsonTree";
import styles from "@/features/requests/detail/BodyViewer.module.css";
import { SegmentedControl } from "@/shared/ui/SegmentedControl";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";

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
    [key]: toggleSet(memory[key], value),
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
              size={15}
              aria-label="Loading body"
            />
          )}
          <button
            type="button"
            onClick={copyBody}
            disabled={!decodedText?.ok}
            aria-label={bodyCopied ? "Body Source copied" : "Copy decoded Body Source"}
            title={bodyCopied ? "Body Source copied" : "Copy decoded Body Source"}
          >
            {bodyCopied ? (
              <Check size={15} aria-hidden="true" />
            ) : (
              <Clipboard size={15} aria-hidden="true" />
            )}
          </button>
          <button
            type="button"
            onClick={onDownload}
            aria-label="Download original body"
            title="Download original body"
          >
            <Download size={15} aria-hidden="true" />
          </button>
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
    <div className={styles.sourceWrap}>
      {(message || onRenderLarge) && (
        <AlertBanner
          className={styles.bodyNotice}
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

function SseEventList({
  requestId,
  events,
  partial,
  active,
  observedAt,
  timings,
  memory,
  onMemoryChange,
}: {
  requestId: string;
  events: ParsedSseEvent[];
  partial: boolean;
  active: boolean;
  observedAt: string;
  timings: EventTimingIndex | null;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
}) {
  const [copiedEvent, copyEventText] = useClipboardFeedback<number>();
  const listRef = useRef<HTMLDivElement | null>(null);
  const followBottom = useRef(true);
  const timingBySequence = useMemo(
    () => new Map((timings?.events ?? []).map((timing) => [timing.sequence, timing])),
    [timings],
  );
  const presentedEvents = useMemo(() => events.map(presentSseEvent), [events]);
  const listEntries = useMemo(
    () => groupSseEventsWithoutPreview(presentedEvents),
    [presentedEvents],
  );

  useLayoutEffect(() => {
    followBottom.current = true;
  }, [requestId]);

  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && shouldPinSseListToBottom(active, followBottom.current)) {
      list.scrollTop = list.scrollHeight;
    }
  }, [active, events.length, requestId]);

  function copyEvent(event: ParsedSseEvent, parsed: ReturnType<typeof parseJson>) {
    const content = parsed.ok ? stringifyJson(parsed.value, true) : event.data;
    void copyEventText(content, event.sequence);
  }

  return (
    <div className={styles.sseWrap}>
      {timings && timings.state !== "available" && (
        <AlertBanner className={styles.bodyNotice} tone="warning">
          {timings.warning ?? "The SSE Event timing index is incomplete."}
        </AlertBanner>
      )}
      <div
        role="list"
        ref={listRef}
        className={styles.eventList}
        aria-label="SSE Events"
        onScroll={(event) => {
          const element = event.currentTarget;
          followBottom.current =
            element.scrollHeight - element.scrollTop - element.clientHeight <= 24;
        }}
      >
        {listEntries.map((entry) =>
          entry.kind === "event" ? (
            <SseEventCard
              key={entry.item.event.sequence}
              presented={entry.item}
              observedAt={observedAt}
              timing={timingBySequence.get(entry.item.event.sequence)}
              copied={copiedEvent === entry.item.event.sequence}
              memory={memory}
              onMemoryChange={onMemoryChange}
              onCopy={() => copyEvent(entry.item.event, entry.item.parsed)}
            />
          ) : (
            <SseEventRun
              key={`run-${entry.items[0].event.sequence}`}
              entry={entry}
              observedAt={observedAt}
              timingBySequence={timingBySequence}
              copiedEvent={copiedEvent}
              memory={memory}
              onMemoryChange={onMemoryChange}
              onCopy={copyEvent}
            />
          ),
        )}
        {events.length === 0 && <p className={styles.emptyEvents}>No complete SSE Events yet.</p>}
        {partial && (
          <p className={styles.partialEvent}>
            {active ? "Still receiving…" : "Incomplete trailing event"}
          </p>
        )}
      </div>
    </div>
  );
}

function SseEventRun({
  entry,
  observedAt,
  timingBySequence,
  copiedEvent,
  memory,
  onMemoryChange,
  onCopy,
}: {
  entry: Extract<SseListEntry, { kind: "run" }>;
  observedAt: string;
  timingBySequence: Map<number, EventTimingEntry>;
  copiedEvent: number | null;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
  onCopy: (event: ParsedSseEvent, parsed: PresentedSseEvent["parsed"]) => void;
}) {
  const runKey = entry.items[0].event.sequence;
  const open = memory.expandedEventRuns.has(runKey);
  const label = sseEventRunLabel(entry);
  const first = entry.items[0].event.sequence + 1;
  const last = entry.items[entry.items.length - 1].event.sequence + 1;

  return (
    <article role="listitem" className={styles.eventCard}>
      <div className={styles.eventHeader}>
        <button
          type="button"
          className={styles.eventToggle}
          aria-expanded={open}
          aria-label={`${entry.items.length} ${entry.type} events, #${first} to #${last}`}
          onClick={() =>
            onMemoryChange({
              ...memory,
              expandedEventRuns: toggleSet(memory.expandedEventRuns, runKey),
            })
          }
        >
          <span className={styles.eventToggleLead}>
            {open ? (
              <ChevronDown size={15} aria-hidden="true" />
            ) : (
              <ChevronRight size={15} aria-hidden="true" />
            )}
            <strong className={styles.eventRunLabel}>{label}</strong>
          </span>
        </button>
      </div>
      {open && (
        <div
          role="list"
          className={styles.eventRunList}
          aria-label={`${entry.type} events #${first} to #${last}`}
        >
          {entry.items.map((item) => (
            <SseEventCard
              key={item.event.sequence}
              presented={item}
              observedAt={observedAt}
              timing={timingBySequence.get(item.event.sequence)}
              copied={copiedEvent === item.event.sequence}
              memory={memory}
              onMemoryChange={onMemoryChange}
              onCopy={() => onCopy(item.event, item.parsed)}
            />
          ))}
        </div>
      )}
    </article>
  );
}

function SseEventCard({
  presented,
  observedAt,
  timing,
  copied,
  memory,
  onMemoryChange,
  onCopy,
}: {
  presented: PresentedSseEvent;
  observedAt: string;
  timing: EventTimingEntry | undefined;
  copied: boolean;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
  onCopy: () => void;
}) {
  const { event, parsed, types, preview } = presented;
  const open = memory.expandedEvents.has(event.sequence);

  return (
    <article role="listitem" className={styles.eventCard}>
      <div className={styles.eventHeader}>
        <button
          type="button"
          className={styles.eventToggle}
          aria-expanded={open}
          onClick={() =>
            onMemoryChange({
              ...memory,
              expandedEvents: toggleSet(memory.expandedEvents, event.sequence),
            })
          }
        >
          <span className={styles.eventToggleLead}>
            {open ? (
              <ChevronDown size={15} aria-hidden="true" />
            ) : (
              <ChevronRight size={15} aria-hidden="true" />
            )}
            <span className={styles.eventSequence}>#{event.sequence + 1}</span>
            <strong>{types.primary}</strong>
            {types.secondary && <span className={styles.eventSecondary}>{types.secondary}</span>}
          </span>
          {preview && <span className={styles.eventPreview}>{preview}</span>}
        </button>
        <span
          className={styles.eventTime}
          title={timing ? eventAbsoluteTime(observedAt, timing.completed_at_ns) : undefined}
        >
          {timing ? eventRelativeTime(timing.completed_at_ns) : "Time unavailable"}
        </span>
        <button
          type="button"
          className={styles.eventCopy}
          onClick={onCopy}
          aria-label={copied ? "SSE Event data copied" : "Copy SSE Event data"}
          title={copied ? "SSE Event data copied" : "Copy SSE Event data"}
        >
          {copied ? (
            <Check size={14} aria-hidden="true" />
          ) : (
            <Clipboard size={14} aria-hidden="true" />
          )}
        </button>
      </div>
      {open && (
        <div className={styles.eventBody}>
          {parsed.ok ? (
            <JsonTree
              compact
              value={parsed.value}
              pathPrefix={`$event/${event.sequence}`}
              expanded={memory.expandedNodes}
              expandedStrings={memory.expandedStrings}
              onToggle={(path) =>
                onMemoryChange({
                  ...memory,
                  expandedNodes: toggleSet(memory.expandedNodes, path),
                })
              }
              onToggleString={(path) =>
                onMemoryChange({
                  ...memory,
                  expandedStrings: toggleSet(memory.expandedStrings, path),
                })
              }
            />
          ) : (
            <pre>{event.data || "(empty data)"}</pre>
          )}
        </div>
      )}
    </article>
  );
}

function BodyState({ children, loading = false }: { children: ReactNode; loading?: boolean }) {
  return (
    <div className={styles.bodyState} role="status">
      {loading && (
        <LoaderCircle className={`${styles.loading} spin`} size={16} aria-hidden="true" />
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

function toggleSet<T>(values: Set<T>, value: T): Set<T> {
  const next = new Set(values);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}
