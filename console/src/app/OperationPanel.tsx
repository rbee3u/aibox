import { Check, ChevronDown, ChevronUp, CircleStop, LoaderCircle, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { Operation, OperationApi } from "@/api/operations";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { messageOf } from "@/shared/lib/errors";
import styles from "@/app/OperationPanel.module.css";

interface OperationPanelProps {
  api: OperationApi;
  operation: Operation;
  connection?: "connecting" | "connected" | "reconnecting";
  onOperation: (operation: Operation) => void;
  onDismiss: () => void;
  onExpandedChange?: (expanded: boolean) => void;
}

export function OperationPanel(props: OperationPanelProps) {
  return (
    <OperationPanelContent key={`${props.operation.id}:${props.operation.state}`} {...props} />
  );
}

function OperationPanelContent({
  api,
  operation,
  connection = "connected",
  onOperation,
  onDismiss,
  onExpandedChange,
}: OperationPanelProps) {
  const [expanded, setExpanded] = useState(operation.state !== "succeeded");
  const [cancelRequested, setCancelRequested] = useState(false);
  const [panelError, setPanelError] = useState<string | null>(null);
  useEffect(() => {
    onExpandedChange?.(expanded);
  }, [expanded, onExpandedChange]);
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
      className={`${styles.operationPanel} ${expanded ? styles.operationPanelExpanded : ""}`}
      aria-label="Management Operation"
    >
      <header>
        <div>
          {operation.state === "running" ? (
            <LoaderCircle className="spin" size={16} />
          ) : operation.state === "succeeded" ? (
            <Check size={16} />
          ) : (
            <CircleStop size={16} />
          )}
          <strong>{operation.kind}</strong>
        </div>
        <span aria-live="polite">
          {cancelRequested && operation.state === "running"
            ? "Cancellation requested"
            : operation.state}
        </span>
        {operation.state === "running" && (
          <IconButton
            label={cancelRequested ? "Cancellation requested" : "Cancel operation"}
            disabled={cancelRequested}
            onClick={() => void cancel()}
          >
            <CircleStop size={16} />
          </IconButton>
        )}
        <RefreshButton
          label="Refresh operation"
          iconOnly
          iconSize={15}
          onClick={() => void api.current().then((value) => value && onOperation(value))}
        />
        <IconButton
          label={expanded ? "Collapse operation" : "Expand operation"}
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronDown size={15} /> : <ChevronUp size={15} />}
        </IconButton>
        {operation.state !== "running" && (
          <IconButton label="Dismiss operation" onClick={onDismiss}>
            <X size={15} />
          </IconButton>
        )}
      </header>
      {expanded && (
        <>
          {operation.first_sequence > 0 && (
            <AlertBanner variant="strip" tone="warning" className={styles.operationGap}>
              Earlier log output was truncated; showing entries from #{operation.first_sequence}.
            </AlertBanner>
          )}
          <pre>
            {operation.logs.map((entry) => entry.message).join("\n") ||
              operation.result ||
              "Connected · waiting for output"}
          </pre>
          {panelError && (
            <AlertBanner variant="strip" tone="danger" className={styles.operationError}>
              {panelError}
            </AlertBanner>
          )}
          <footer>
            <span>
              {operation.state !== "running"
                ? "Terminal state"
                : connection === "connected"
                  ? "Live updates connected"
                  : connection === "reconnecting"
                    ? "Reconnecting to live updates"
                    : "Connecting to live updates"}
            </span>
            {operation.result && <strong>{operation.result}</strong>}
          </footer>
        </>
      )}
    </section>
  );
}
