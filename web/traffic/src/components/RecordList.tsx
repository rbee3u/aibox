import {
  Check,
  ChevronLeft,
  ChevronRight,
  Clock3,
  ListChecks,
  LoaderCircle,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useEffect, useRef } from "react";
import type { RecordSummary } from "../types";
import { compactDuration, formatTimestamp, recordUrl } from "../utils";
import styles from "./RecordList.module.css";
import { RecordStatus } from "./RecordStatus";

interface RecordListProps {
  records: RecordSummary[];
  total: number;
  page: number;
  hasPrevious: boolean;
  hasNext: boolean;
  selectionMode: boolean;
  selected: Set<string>;
  currentId: string | null;
  onEnterSelection: () => void;
  onExitSelection: () => void;
  onTogglePage: () => void;
  onToggle: (id: string) => void;
  onSelect: (id: string) => void;
  onPrevious: () => void;
  onNext: () => void;
  loading: boolean;
  refreshing: boolean;
  deletableCount: number;
  onRefresh: () => void;
  onDeleteSelected: () => void;
  onDeleteAll: () => void;
  onDeleteRecord: (id: string) => void;
  deletingRecordId: string | null;
  deletionBusy: boolean;
  focusAfterDelete: string | null | undefined;
  onFocusAfterDelete: () => void;
}

export function RecordList({
  records,
  total,
  page,
  hasPrevious,
  hasNext,
  selectionMode,
  selected,
  currentId,
  onEnterSelection,
  onExitSelection,
  onTogglePage,
  onToggle,
  onSelect,
  onPrevious,
  onNext,
  loading,
  refreshing,
  deletableCount,
  onRefresh,
  onDeleteSelected,
  onDeleteAll,
  onDeleteRecord,
  deletingRecordId,
  deletionBusy,
  focusAfterDelete,
  onFocusAfterDelete,
}: RecordListProps) {
  const deletable = records.filter((record) => record.state !== "active");
  const selectedOnPage = deletable.filter((record) => selected.has(record.id)).length;
  const pageSelected = deletable.length > 0 && selectedOnPage === deletable.length;
  const refreshButton = useRef<HTMLButtonElement>(null);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());

  useEffect(() => {
    if (focusAfterDelete === undefined) return;
    const target =
      focusAfterDelete === null
        ? refreshButton.current
        : deleteButtons.current.get(focusAfterDelete);
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterDelete();
  }, [deletionBusy, focusAfterDelete, onFocusAfterDelete, records]);

  return (
    <aside className={styles.panel} aria-label="Traffic records">
      <div className={styles.listHeader}>
        {selectionMode ? (
          <>
            <button
              type="button"
              className={styles.pageSelection}
              onClick={onTogglePage}
              disabled={deletable.length === 0}
            >
              {pageSelected ? "Clear page" : "Select page"}
            </button>
            <div className={styles.selectionActions}>
              <span className={styles.selectionCount}>{selected.size} selected</span>
              <button
                type="button"
                className={styles.deleteSelected}
                onClick={onDeleteSelected}
                disabled={selected.size === 0 || deletionBusy}
                aria-label="Delete selected"
                title="Delete selected"
              >
                <Trash2 size={14} aria-hidden="true" />
                <span className={styles.deleteSelectedLabel}>Delete selected</span>
              </button>
              <button type="button" className={styles.cancelSelection} onClick={onExitSelection}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            <h2 className={styles.title}>Traffic records</h2>
            <div className={styles.headerActions}>
              <button
                ref={refreshButton}
                type="button"
                className={styles.refreshButton}
                onClick={onRefresh}
                disabled={refreshing}
                aria-label={refreshing ? "Refreshing traffic records" : "Refresh traffic records"}
                aria-busy={refreshing}
              >
                <RefreshCw
                  className={refreshing ? styles.refreshing : undefined}
                  size={14}
                  aria-hidden="true"
                />
                Refresh
              </button>
              <button
                type="button"
                className={styles.selectRecords}
                onClick={onEnterSelection}
                disabled={deletableCount === 0 || loading || deletionBusy}
              >
                <ListChecks size={14} aria-hidden="true" /> Select
              </button>
              <button
                type="button"
                className={styles.deleteAll}
                onClick={onDeleteAll}
                disabled={deletableCount === 0 || deletionBusy}
              >
                <Trash2 size={14} aria-hidden="true" /> Delete all
              </button>
            </div>
          </>
        )}
      </div>
      <div className={styles.records} aria-live="polite">
        {records.length === 0 ? (
          <div className={styles.empty}>
            <Clock3 size={22} aria-hidden="true" />
            <p>No traffic recorded yet.</p>
          </div>
        ) : (
          records.map((record) => {
            const [host, path] = recordUrl(record);
            const active = record.state === "active";
            const checked = selected.has(record.id);
            const first = compactDuration(record.ttfb_ms);
            const totalDuration = compactDuration(record.total_ms);
            const timingDescription = `First ${first}; Total ${totalDuration}`;
            const timingDescriptionId = `record-timing-${record.id}`;
            return (
              <div
                key={record.id}
                className={[
                  styles.record,
                  currentId === record.id ? styles.current : "",
                  selectionMode ? styles.selectionRecord : "",
                  checked ? styles.selected : "",
                  selectionMode && active ? styles.unselectable : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <button
                  type="button"
                  className={styles.rowButton}
                  disabled={selectionMode && active}
                  aria-label={
                    selectionMode
                      ? `${checked ? "Deselect" : "Select"} ${record.method} ${host}`
                      : `${record.method} ${host}`
                  }
                  aria-pressed={selectionMode ? checked : undefined}
                  aria-describedby={timingDescriptionId}
                  onClick={() => (selectionMode ? onToggle(record.id) : onSelect(record.id))}
                >
                  <span className={styles.method}>{record.method}</span>
                  <span className={styles.main}>
                    <strong>{host}</strong>
                    <span>{path}</span>
                  </span>
                  <span className={styles.side}>
                    <RecordStatus
                      status={record.status}
                      outcome={record.outcome}
                      state={record.state}
                      compact
                    />
                    <span className={styles.timestamp}>{formatTimestamp(record.started_at)}</span>
                    <span className={styles.timing} title={timingDescription}>
                      {first} / {totalDuration}
                    </span>
                    <span id={timingDescriptionId} className={styles.visuallyHidden}>
                      {timingDescription}
                    </span>
                  </span>
                  {selectionMode && (
                    <span className={styles.selectionIndicator} aria-hidden="true">
                      {checked && <Check size={16} strokeWidth={3} />}
                    </span>
                  )}
                </button>
                {!selectionMode && (
                  <span
                    className={styles.deleteSlot}
                    title={active ? "Active records cannot be deleted" : undefined}
                  >
                    <button
                      ref={(element) => {
                        if (element) deleteButtons.current.set(record.id, element);
                        else deleteButtons.current.delete(record.id);
                      }}
                      type="button"
                      className={styles.deleteRecord}
                      onClick={() => onDeleteRecord(record.id)}
                      disabled={active || deletionBusy}
                      aria-label={
                        active
                          ? `Cannot delete active ${record.method} ${host}`
                          : deletingRecordId === record.id
                            ? `Deleting ${record.method} ${host}`
                            : `Delete ${record.method} ${host}`
                      }
                      aria-busy={deletingRecordId === record.id}
                      title={active ? undefined : `Delete ${record.method} ${host}`}
                    >
                      {deletingRecordId === record.id ? (
                        <LoaderCircle className={styles.deleting} size={15} aria-hidden="true" />
                      ) : (
                        <Trash2 size={15} aria-hidden="true" />
                      )}
                    </button>
                  </span>
                )}
              </div>
            );
          })
        )}
      </div>
      <nav className={styles.pagination} aria-label="Record pages">
        <button type="button" onClick={onPrevious} disabled={!hasPrevious || loading}>
          <ChevronLeft size={15} aria-hidden="true" /> Previous
        </button>
        <span>
          Page {page + 1} · {records.length} shown · {total} total
        </span>
        <button type="button" onClick={onNext} disabled={!hasNext || loading}>
          Next <ChevronRight size={15} aria-hidden="true" />
        </button>
      </nav>
    </aside>
  );
}
