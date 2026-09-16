import { ChevronDown, ChevronUp, CircleStop, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Operation, OperationApi } from "@/api/operations";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { compactDuration } from "@/shared/lib/format";
import { messageOf } from "@/shared/lib/errors";
import { operationElapsedMs, operationPresentation } from "@/app/operationPresentation";
import styles from "@/app/OperationPanel.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

interface OperationPanelProps {
  api: OperationApi;
  operation: Operation;
  connection?: "connecting" | "connected" | "reconnecting";
  onOperation: (operation: Operation) => void;
  onDismiss: () => void;
  /** Reports the space the fixed panel occupies so the shell can reserve it. */
  onHeightChange?: (height: number) => void;
}

export function OperationPanel(props: OperationPanelProps) {
  return (
    <OperationPanelContent key={`${props.operation.id}:${props.operation.state}`} {...props} />
  );
}

/**
 * Reports the panel's own height to the shell. The collapsed bar wraps its
 * header at narrow widths and with a long Operation kind, so the space to
 * reserve is measured rather than restated as a constant that can drift.
 */
function useReportedHeight(onHeightChange: ((height: number) => void) | undefined) {
  const panelRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const element = panelRef.current;
    if (!element || !onHeightChange) return;
    const observer = new ResizeObserver(() => {
      onHeightChange(element.getBoundingClientRect().height);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      onHeightChange(0);
    };
  }, [onHeightChange]);
  return panelRef;
}

/**
 * How long the Operation has been running. A running Operation retimes every
 * second, because "is it stuck" is the question a multi-minute install invites
 * and a static label cannot answer it.
 */
function useElapsedLabel(operation: Operation): string | null {
  const running = operation.state === "running";
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  // A finished Operation carries `ended_at`, so the clock only advances a
  // running one. A finished Operation missing its end reports nothing rather
  // than presenting the time since mount as a total.
  const elapsed = operationElapsedMs(operation, now);
  if (elapsed === null) return null;
  if (running) return compactDuration(elapsed);
  return operation.ended_at ? `took ${compactDuration(elapsed)}` : null;
}

/**
 * Keeps the newest log line in view while the reader is at the bottom.
 *
 * Stickiness is decided against the geometry the box had before this frame
 * arrived, not from scroll events: assigning `scrollTop` emits its own scroll
 * event, which lands after the next batch of log lines has already grown the
 * box and so reports the reader as having scrolled away when they had not.
 * Scrolling up is a deliberate act of reading history, so a new frame must not
 * yank the view back down; returning to the bottom re-arms following.
 */
function useLogFollow() {
  const logRef = useRef<HTMLPreElement>(null);
  const measured = useRef<HTMLPreElement | null>(null);
  const previousScrollHeight = useRef(0);
  useEffect(() => {
    const element = logRef.current;
    if (!element) {
      measured.current = null;
      return;
    }
    // A newly expanded panel opens on the newest output rather than the oldest.
    if (measured.current !== element) {
      measured.current = element;
      previousScrollHeight.current = 0;
    }
    const previousBottom = previousScrollHeight.current - element.clientHeight;
    if (element.scrollTop >= previousBottom - 8) {
      element.scrollTop = element.scrollHeight;
    }
    previousScrollHeight.current = element.scrollHeight;
  });
  return logRef;
}

function OperationPanelContent({
  api,
  operation,
  connection = "connected",
  onOperation,
  onDismiss,
  onHeightChange,
}: OperationPanelProps) {
  const [expanded, setExpanded] = useState(operation.state !== "succeeded");
  const [cancelRequested, setCancelRequested] = useState(false);
  const [panelError, setPanelError] = useState<string | null>(null);
  const { label, tone, icon: StateIcon } = operationPresentation(operation.state);
  const resultTone =
    operation.state === "failed"
      ? "danger"
      : operation.state === "succeeded"
        ? "success"
        : "neutral";
  const elapsedLabel = useElapsedLabel(operation);
  const logRef = useLogFollow();
  const panelRef = useReportedHeight(onHeightChange);
  async function cancel() {
    if (cancelRequested) return;
    setCancelRequested(true);
    setPanelError(null);
    try {
      await api.cancel(operation.id);
    } catch (cause) {
      setCancelRequested(false);
      setPanelError(messageOf(cause));
    }
  }
  return (
    <section
      ref={panelRef}
      className={`${styles.operationPanel} ${expanded ? styles.operationPanelExpanded : ""}`}
      aria-label="Management Operation"
    >
      <header>
        <div>
          <StateIcon
            size={iconSize.sm}
            className={`${styles.stateIcon} ${styles[tone]} ${operation.state === "running" ? "spin" : ""}`}
          />
          <strong>{operation.kind}</strong>
        </div>
        <span className={styles.stateGroup}>
          {elapsedLabel && <span className={styles.elapsed}>{elapsedLabel}</span>}
          {/*
           * A requested cancellation is not an observed one: Docker may still be
           * finishing. The status reports only what the Console knows, and reuses
           * the string the disabled Cancel control already carries.
           */}
          <span aria-live="polite" role="status">
            <StatusBadge tone={tone} variant="inline" dot={false}>
              {cancelRequested && operation.state === "running" ? "Cancellation requested" : label}
            </StatusBadge>
          </span>
        </span>
        {operation.state === "running" && (
          <IconButton
            label={cancelRequested ? "Cancellation requested" : "Cancel operation"}
            disabled={cancelRequested}
            onClick={() => void cancel()}
          >
            <CircleStop size={iconSize.sm} />
          </IconButton>
        )}
        <RefreshButton
          label="Refresh operation"
          compactOnNarrow
          iconSize={15}
          onClick={() => void api.current().then((value) => value && onOperation(value))}
        >
          Refresh
        </RefreshButton>
        <IconButton
          label={expanded ? "Collapse operation" : "Expand operation"}
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronDown size={iconSize.xs} /> : <ChevronUp size={iconSize.xs} />}
        </IconButton>
        {operation.state !== "running" && (
          <IconButton label="Dismiss operation" onClick={onDismiss}>
            <X size={iconSize.xs} />
          </IconButton>
        )}
      </header>
      {expanded && (
        <>
          {/*
           * The result is the Operation's conclusion, so it reads directly
           * under the state that names it. A failure's reason used to sit in
           * the footer's far-right slot, the one place in the panel that is
           * neither under the title nor beside the log.
           */}
          {operation.result && (
            <AlertBanner
              variant="strip"
              tone={resultTone}
              role="status"
              className={styles.operationResult}
            >
              {operation.result}
            </AlertBanner>
          )}
          {operation.first_sequence > 0 && (
            <AlertBanner variant="strip" tone="warning" className={styles.operationGap}>
              Earlier log output was truncated; showing entries from #{operation.first_sequence}.
            </AlertBanner>
          )}
          <pre ref={logRef}>
            {operation.logs.map((entry) => entry.message).join("\n") ||
              "Connected · waiting for output"}
          </pre>
          {panelError && (
            <AlertBanner variant="strip" tone="danger" className={styles.operationError}>
              {panelError}
            </AlertBanner>
          )}
          <footer>
            {operation.state !== "running"
              ? "Finished"
              : connection === "connected"
                ? "Live updates connected"
                : connection === "reconnecting"
                  ? "Reconnecting to live updates"
                  : "Connecting to live updates"}
          </footer>
        </>
      )}
    </section>
  );
}
