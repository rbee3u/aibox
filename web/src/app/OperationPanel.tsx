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
 * Measure the fixed panel because narrow layouts and long labels wrap its header.
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

function useElapsedLabel(operation: Operation): string | null {
  const running = operation.state === "running";
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  const elapsed = operationElapsedMs(operation, now);
  if (elapsed === null) return null;
  if (running) return compactDuration(elapsed);
  return operation.ended_at ? `took ${compactDuration(elapsed)}` : null;
}

/**
 * Follow new output only when the reader was at the previous bottom. Compare
 * against pre-update geometry: programmatic scroll events arrive after content
 * grows and can falsely appear to be a user scrolling away.
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
          {/* Docker may still be running after cancellation is requested. */}
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
