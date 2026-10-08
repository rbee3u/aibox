import styles from "@/features/requests/detail/SseEventList.module.css";
import bodySharedStyles from "@/features/requests/detail/bodyShared.module.css";
import { Check, ChevronDown, ChevronRight, Clipboard } from "lucide-react";
import { useLayoutEffect, useMemo, useRef } from "react";

import {
  eventAbsoluteTime,
  eventRelativeTime,
  groupSseEventsWithoutPreview,
  parseJson,
  presentSseEvent,
  shouldPinSseListToBottom,
  sseEventRunLabel,
  sseTimingNotice,
  stringifyJson,
  type ParsedSseEvent,
  type PresentedSseEvent,
  type SseListEntry,
} from "@/features/requests/detail/bodyPresentation";
import type { BodyViewMemory } from "@/features/requests/detail/bodyViewMemory";
import type { EventTimingEntry, EventTimingIndex } from "@/api/requests";

import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";

import { JsonTree } from "@/features/requests/detail/JsonTree";

import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { iconSize } from "@/shared/icons/iconSizes";

import { toggleBodyViewSet } from "@/features/requests/detail/bodyViewMemory";
export function SseEventList({
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

  const timingNotice = sseTimingNotice(timings);

  return (
    <div className={bodySharedStyles.sseWrap}>
      {timingNotice && (
        <AlertBanner className={bodySharedStyles.bodyNotice} tone="warning">
          {timingNotice}
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
              expandedEventRuns: toggleBodyViewSet(memory.expandedEventRuns, runKey),
            })
          }
        >
          <span className={styles.eventToggleLead}>
            {open ? (
              <ChevronDown size={iconSize.xs} aria-hidden="true" />
            ) : (
              <ChevronRight size={iconSize.xs} aria-hidden="true" />
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
              expandedEvents: toggleBodyViewSet(memory.expandedEvents, event.sequence),
            })
          }
        >
          <span className={styles.eventToggleLead}>
            {open ? (
              <ChevronDown size={iconSize.xs} aria-hidden="true" />
            ) : (
              <ChevronRight size={iconSize.xs} aria-hidden="true" />
            )}
            <span className={styles.eventSequence}>#{event.sequence + 1}</span>
            <strong>{types.primary}</strong>
            {types.secondary && <span className={styles.eventSecondary}>{types.secondary}</span>}
          </span>
          {preview && <span className={styles.eventPreview}>{preview}</span>}
        </button>
        {timing && (
          <span
            className={styles.eventTime}
            title={eventAbsoluteTime(observedAt, timing.completed_at_ns)}
          >
            {eventRelativeTime(timing.completed_at_ns)}
          </span>
        )}
        <button
          type="button"
          className={styles.eventCopy}
          onClick={onCopy}
          aria-label={copied ? "SSE Event data copied" : "Copy SSE Event data"}
          title={copied ? "SSE Event data copied" : "Copy SSE Event data"}
        >
          {copied ? (
            <Check size={iconSize.xs} aria-hidden="true" />
          ) : (
            <Clipboard size={iconSize.xs} aria-hidden="true" />
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
                  expandedNodes: toggleBodyViewSet(memory.expandedNodes, path),
                })
              }
              onToggleString={(path) =>
                onMemoryChange({
                  ...memory,
                  expandedStrings: toggleBodyViewSet(memory.expandedStrings, path),
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
