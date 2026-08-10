import { Check, ChevronDown, ChevronRight, Clipboard, Download, LoaderCircle } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  bodyComplete,
  bodyMediaType,
  contentCoding,
  decodeUtf8,
  eventAbsoluteTime,
  eventRelativeTime,
  isJsonMediaType,
  isSseResponse,
  parseJson,
  parseSse,
  sseEventTypes,
  shouldDeferPretty,
  stringifyJson,
  type ParsedSseEvent,
} from "../bodyPresentation";
import type { BodyViewMemory } from "../bodyViewMemory";
import type {
  BodyKind,
  BodyLoadStatus,
  DecodedBodyState,
  EventTimingIndex,
  RecordDetail,
} from "../types";
import { bytes, concatChunks, hex } from "../utils";
import { JsonTree } from "./JsonTree";
import styles from "./RecordDetail.module.css";

interface BodyViewerProps {
  kind: BodyKind;
  detail: RecordDetail;
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
  const [bodyCopied, setBodyCopied] = useState(false);
  const copiedTimer = useRef<number | undefined>(undefined);
  const headers = kind === "request" ? detail.request.headers : (detail.response?.headers ?? []);
  const coding = contentCoding(headers);
  const original = useMemo(() => concatChunks(bodyChunks), [bodyChunks]);
  const complete = bodyComplete(detail, kind);
  const sourceBytes = useMemo(
    () =>
      coding.kind === "identity"
        ? original
        : coding.kind === "zstd" && decoded.status === "loaded"
          ? decoded.bytes
          : null,
    [coding.kind, decoded.bytes, decoded.status, original],
  );
  const decodedText = useMemo(() => (sourceBytes ? decodeUtf8(sourceBytes) : null), [sourceBytes]);
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
  const pendingPretty =
    (declaredJson && !complete) ||
    (coding.kind === "zstd" && (decoded.status === "waiting" || decoded.status === "loading"));
  const prettyAvailable = sse ? parsedEvents !== null : jsonPretty;
  const resolvedMode =
    memory.mode === "pretty" && (prettyAvailable || pendingPretty) ? "pretty" : "source";
  const originalSize = kind === "request" ? detail.request_body_bytes : detail.response_body_bytes;
  const decodedSize = coding.kind === "zstd" ? sourceBytes?.length : undefined;

  useEffect(
    () => () => {
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  async function copyBody() {
    if (!decodedText?.ok) return;
    try {
      await navigator.clipboard.writeText(decodedText.text);
      setBodyCopied(true);
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setBodyCopied(false), 1400);
    } catch {
      setBodyCopied(false);
    }
  }

  const updateSet = (key: "expandedNodes" | "expandedStrings", value: string): BodyViewMemory => ({
    ...memory,
    [key]: toggleSet(memory[key], value),
  });

  return (
    <>
      <div className={styles.sectionTitle}>
        <h2>
          Body <span>· {bytes(originalSize)}</span>
          {decodedSize !== undefined && decodedSize !== originalSize && (
            <span>· {bytes(decodedSize)} decoded</span>
          )}
        </h2>
        <div className={styles.bodyActions}>
          <div className={styles.viewToggle} role="group" aria-label={`${kind} body view`}>
            <button
              type="button"
              className={resolvedMode === "pretty" ? styles.activeView : ""}
              aria-pressed={resolvedMode === "pretty"}
              disabled={!prettyAvailable && !pendingPretty}
              onClick={() => onMemoryChange({ ...memory, mode: "pretty" })}
            >
              Pretty
            </button>
            <button
              type="button"
              className={resolvedMode === "source" ? styles.activeView : ""}
              aria-pressed={resolvedMode === "source"}
              onClick={() => onMemoryChange({ ...memory, mode: "source" })}
            >
              Source
            </button>
          </div>
          {loadingBody && (
            <LoaderCircle className={styles.loading} size={15} aria-label="Loading body" />
          )}
          <button
            type="button"
            onClick={() => void copyBody()}
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
      {bodyStatus === "error" ? (
        <BodyState>Original Body unavailable.</BodyState>
      ) : bodyStatus === "idle" || (bodyStatus === "loading" && original.length === 0) ? (
        <BodyState loading>Loading Body…</BodyState>
      ) : resolvedMode === "pretty" && pendingPretty ? (
        <BodyState loading>
          {coding.kind === "zstd"
            ? "Waiting for the complete zstd Body before decoding…"
            : "Waiting for the complete JSON Body…"}
        </BodyState>
      ) : resolvedMode === "pretty" && parsedEvents ? (
        <SseEventList
          events={parsedEvents.events}
          partial={parsedEvents.hasPartialTail}
          active={detail.state === "active"}
          observedAt={detail.request.started_at}
          timings={timings}
          memory={memory}
          onMemoryChange={onMemoryChange}
        />
      ) : resolvedMode === "pretty" && parsedJson?.ok ? (
        <JsonTree
          value={parsedJson.value}
          expanded={memory.expandedNodes}
          expandedStrings={memory.expandedStrings}
          onToggle={(path) => onMemoryChange(updateSet("expandedNodes", path))}
          onToggleString={(path) => onMemoryChange(updateSet("expandedStrings", path))}
        />
      ) : (
        <SourceView
          original={original}
          decodedText={decodedText}
          coding={coding.kind}
          message={sourceMessage({
            coding: coding.kind,
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
            large && !memory.renderLarge
              ? () => onMemoryChange({ ...memory, renderLarge: true, mode: "pretty" })
              : undefined
          }
        />
      )}
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
  coding: "identity" | "zstd" | "unsupported";
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
        <div className={styles.bodyNotice} role="status">
          <span>{message}</span>
          {onRenderLarge && (
            <button type="button" onClick={onRenderLarge}>
              Render Pretty
            </button>
          )}
        </div>
      )}
      {coding !== "identity" && !decodedText && (
        <div className={styles.encodedLabel}>Encoded original bytes</div>
      )}
      <pre className={styles.body}>{source}</pre>
    </div>
  );
}

function SseEventList({
  events,
  partial,
  active,
  observedAt,
  timings,
  memory,
  onMemoryChange,
}: {
  events: ParsedSseEvent[];
  partial: boolean;
  active: boolean;
  observedAt: string;
  timings: EventTimingIndex | null;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
}) {
  const [copiedEvent, setCopiedEvent] = useState<number | null>(null);
  const copiedTimer = useRef<number | undefined>(undefined);
  const listRef = useRef<HTMLDivElement | null>(null);
  const followBottom = useRef(true);
  const timingBySequence = useMemo(
    () => new Map((timings?.events ?? []).map((timing) => [timing.sequence, timing])),
    [timings],
  );

  useEffect(
    () => () => {
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
    },
    [],
  );
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && followBottom.current) list.scrollTop = list.scrollHeight;
  }, [events.length]);

  async function copyEvent(event: ParsedSseEvent) {
    const parsed = parseJson(event.data);
    const content = parsed.ok ? stringifyJson(parsed.value, true) : event.data;
    try {
      await navigator.clipboard.writeText(content);
      setCopiedEvent(event.sequence);
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopiedEvent(null), 1400);
    } catch {
      setCopiedEvent(null);
    }
  }

  return (
    <div className={styles.sseWrap}>
      {timings && timings.state !== "available" && (
        <div className={styles.bodyNotice} role="status">
          {timings.warning ?? "The SSE Event timing index is incomplete."}
        </div>
      )}
      <div
        ref={listRef}
        className={styles.eventList}
        aria-label="SSE Events"
        onScroll={(event) => {
          const element = event.currentTarget;
          followBottom.current =
            element.scrollHeight - element.scrollTop - element.clientHeight <= 24;
        }}
      >
        {events.map((event) => {
          const open = memory.expandedEvents.has(event.sequence);
          const types = sseEventTypes(event);
          const timing = timingBySequence.get(event.sequence);
          const parsed = parseJson(event.data);
          return (
            <article className={styles.eventCard} key={event.sequence}>
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
                  {open ? (
                    <ChevronDown size={15} aria-hidden="true" />
                  ) : (
                    <ChevronRight size={15} aria-hidden="true" />
                  )}
                  <span className={styles.eventSequence}>#{event.sequence + 1}</span>
                  <strong>{types.primary}</strong>
                  {types.secondary && (
                    <span className={styles.eventSecondary}>{types.secondary}</span>
                  )}
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
                  onClick={() => void copyEvent(event)}
                  aria-label={
                    copiedEvent === event.sequence ? "SSE Event data copied" : "Copy SSE Event data"
                  }
                  title={
                    copiedEvent === event.sequence ? "SSE Event data copied" : "Copy SSE Event data"
                  }
                >
                  {copiedEvent === event.sequence ? (
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
        })}
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

function BodyState({
  children,
  loading = false,
}: {
  children: React.ReactNode;
  loading?: boolean;
}) {
  return (
    <div className={styles.bodyState} role="status">
      {loading && <LoaderCircle className={styles.loading} size={16} aria-hidden="true" />}
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
  coding: "identity" | "zstd" | "unsupported";
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
  if (coding === "unsupported" || decoded.status === "unsupported") return decoded.message;
  if (decoded.status === "error") return decoded.message;
  if (coding === "zstd" && decoded.status !== "loaded") return decoded.message;
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
