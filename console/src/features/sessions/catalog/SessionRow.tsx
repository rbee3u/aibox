import { AlertTriangle, Check, LoaderCircle, Trash2 } from "lucide-react";
import { sessionListCopy } from "@/features/sessions/sessionListCopy";
import {
  accessibleSessionSource,
  visibleSessionListSource,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { formatTimestamp } from "@/shared/lib/format";
import { IconButton } from "@/shared/ui/IconButton";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;

interface SessionRowProps {
  row: SourcedSession;
  current: boolean;
  selectionMode: boolean;
  selected: boolean;
  deleting: boolean;
  /** Any destructive work in flight, including another row's deletion. */
  mutationBusy: boolean;
  deletionBusy: boolean;
  loadingList: boolean;
  /** A traversal error makes the listed rows unsafe to act on. */
  unsafeView: boolean;
  /** When the catalog already names one Tenant and one Agent, omit the source. */
  showSource: boolean;
  onOpen: () => void;
  onToggle: () => void;
  onDelete: () => void;
  registerRow: (element: HTMLButtonElement | null) => void;
  registerDelete: (element: HTMLButtonElement | null) => void;
}

/** One Session catalog row: title, source, preview, and its danger delete action. */
export function SessionRow({
  row,
  current,
  selectionMode,
  selected,
  deleting,
  mutationBusy,
  deletionBusy,
  loadingList,
  unsafeView,
  showSource,
  onOpen,
  onToggle,
  onDelete,
  registerRow,
  registerDelete,
}: SessionRowProps) {
  const copy = sessionListCopy(row.title, row.latest_message);
  const accessibleSource = accessibleSessionSource(row.source);
  return (
    <div
      className={[
        styles.sessionRow,
        current ? styles.currentSessionRow : "",
        selectionMode ? styles.sessionSelectionRow : "",
        selected ? styles.sessionRowSelected : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <button
        ref={registerRow}
        type="button"
        className={styles.sessionRowMain}
        aria-label={
          selectionMode
            ? `${selected ? "Deselect" : "Select"} ${copy.headline}, ${accessibleSource}`
            : `${copy.headline}, ${accessibleSource}`
        }
        aria-pressed={selectionMode ? selected : undefined}
        disabled={deletionBusy || loadingList}
        onClick={() => (selectionMode ? onToggle() : onOpen())}
      >
        <SessionIcon size={16} data-icon="session-record" aria-hidden="true" />
        <span>
          <strong title={copy.headline}>{copy.headline}</strong>
          <small className={styles.sessionRowMetadata}>
            {showSource ? <span>{visibleSessionListSource(row.source)}</span> : null}
            <time dateTime={row.start_ts}>{formatTimestamp(row.start_ts)}</time>
          </small>
          {(copy.supporting || copy.emptyPreview) && (
            <small className={styles.sessionRowPreview} title={copy.supporting ?? ""}>
              {copy.supporting || "No readable conversation content"}
            </small>
          )}
        </span>
        {row.warnings.length > 0 && (
          <span
            className={styles.sessionRowWarning}
            role="img"
            aria-label={`Session has ${row.warnings.length} Transcript warning${row.warnings.length === 1 ? "" : "s"}`}
            title={row.warnings.join("\n")}
          >
            <AlertTriangle size={14} aria-hidden="true" />
          </span>
        )}
        {selectionMode && (
          <span className={styles.sessionSelectionIndicator} aria-hidden="true">
            {selected && <Check size={15} strokeWidth={3} />}
          </span>
        )}
      </button>
      {!selectionMode && (
        <IconButton
          ref={registerDelete}
          className={styles.sessionDelete}
          tone="dangerQuiet"
          label={
            deleting
              ? `Deleting Session ${row.display_id} from ${accessibleSource}`
              : `Delete Session ${row.display_id} from ${accessibleSource}`
          }
          aria-busy={deleting}
          disabled={unsafeView || mutationBusy || loadingList}
          onClick={onDelete}
        >
          {deleting ? (
            <LoaderCircle className="spin" size={15} aria-hidden="true" />
          ) : (
            <Trash2 size={15} aria-hidden="true" />
          )}
        </IconButton>
      )}
    </div>
  );
}
