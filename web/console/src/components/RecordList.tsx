import {
  Check,
  ChevronLeft,
  ChevronRight,
  Inbox,
  ListChecks,
  LoaderCircle,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useEffect, useRef } from "react";
import { elapsedNsMs, resolveRequestedEffective } from "../summary";
import type { RecordSummary } from "../types";
import { compactDuration, formatTimestamp, recordUrl } from "../utils";
import styles from "./RecordList.module.css";
import { RecordStatus } from "./RecordStatus";
import { assessmentIssueText, assessmentPresentation } from "./statusPresentation";

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
  onDeleteRecord: (id: string) => void;
  deletingRecordId: string | null;
  deletionBusy: boolean;
  focusAfterDelete: string | null | undefined;
  onFocusAfterDelete: () => void;
  focusAfterInspection: string | null | undefined;
  onFocusAfterInspection: () => void;
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
  onDeleteRecord,
  deletingRecordId,
  deletionBusy,
  focusAfterDelete,
  onFocusAfterDelete,
  focusAfterInspection,
  onFocusAfterInspection,
}: RecordListProps) {
  const deletable = records.filter((record) => record.state !== "active");
  const selectedOnPage = deletable.filter((record) => selected.has(record.id)).length;
  const pageSelected = deletable.length > 0 && selectedOnPage === deletable.length;
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const recordButtons = useRef(new Map<string, HTMLButtonElement>());

  useEffect(() => {
    if (focusAfterDelete === undefined) return;
    const preferred =
      focusAfterDelete === null
        ? refreshButton.current
        : deleteButtons.current.get(focusAfterDelete);
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterDelete();
  }, [deletionBusy, focusAfterDelete, onFocusAfterDelete, records]);

  useEffect(() => {
    if (focusAfterInspection === undefined) return;
    const preferred =
      focusAfterInspection === null
        ? refreshButton.current
        : recordButtons.current.get(focusAfterInspection);
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterInspection();
  }, [focusAfterInspection, onFocusAfterInspection, records]);

  useEffect(() => {
    if (selectionMode || !focusSelectAfterExit.current) return;
    focusSelectAfterExit.current = false;
    selectButton.current?.focus();
  }, [selectionMode]);

  return (
    <aside className={styles.panel} aria-label="Request Record list">
      <div className={`${styles.listHeader} ${selectionMode ? styles.selectionHeader : ""}`}>
        {selectionMode && (
          <button
            type="button"
            className={styles.cancelSelection}
            onClick={() => {
              focusSelectAfterExit.current = true;
              onExitSelection();
            }}
          >
            Cancel
          </button>
        )}
        <div className={selectionMode ? styles.selectionActions : styles.headerActions}>
          {selectionMode ? (
            <span className={styles.selectionCount}>{selected.size} selected</span>
          ) : (
            <button
              ref={refreshButton}
              data-dialog-focus-fallback="true"
              type="button"
              className={styles.refreshButton}
              onClick={onRefresh}
              disabled={refreshing || deletionBusy}
              aria-label={
                refreshing ? "Refreshing Request Record list" : "Refresh Request Record list"
              }
              aria-busy={refreshing}
            >
              <RefreshCw
                className={refreshing ? styles.refreshing : undefined}
                size={14}
                aria-hidden="true"
              />
              Refresh
            </button>
          )}
          {selectionMode && (
            <>
              <button
                type="button"
                className={styles.pageSelection}
                onClick={onTogglePage}
                disabled={deletable.length === 0}
              >
                {pageSelected ? "Clear page" : "Select page"}
              </button>
              <button
                type="button"
                className={styles.deleteSelected}
                onClick={(event) => {
                  event.currentTarget.focus();
                  onDeleteSelected();
                }}
                disabled={selected.size === 0 || deletionBusy}
                aria-label="Delete selected"
                title="Delete selected"
              >
                <Trash2 size={14} aria-hidden="true" />
                Delete selected
              </button>
            </>
          )}
          {!selectionMode && (
            <button
              ref={selectButton}
              type="button"
              className={styles.selectRecords}
              onClick={onEnterSelection}
              disabled={deletableCount === 0 || loading || deletionBusy}
            >
              <ListChecks size={14} aria-hidden="true" /> Select
            </button>
          )}
        </div>
      </div>
      <div className={styles.records} aria-busy={loading}>
        {records.length === 0 ? (
          <div className={styles.empty}>
            <Inbox size={22} data-icon="request-empty" aria-hidden="true" />
            <p>No request recorded yet.</p>
          </div>
        ) : (
          records.map((record) => {
            const target = recordUrl(record);
            const active = record.state === "active";
            const checked = selected.has(record.id);
            const model = resolveRequestedEffective(record.protocol?.model) ?? "—";
            const reasoningEffort =
              resolveRequestedEffective(record.protocol?.reasoning_effort) ?? "—";
            const firstToken = compactDuration(elapsedNsMs(record.protocol?.first_token_at_ns));
            const totalDuration = compactDuration(record.total_ms);
            const ended = formatTimestamp(record.ended_at ?? "");
            const issue = assessmentPresentation(record.assessment);
            const modelDescription = `Model ${model}; Reasoning effort ${reasoningEffort}`;
            const timingDescription = `First token ${firstToken}; Duration ${totalDuration}`;
            const metadataDescription = [
              modelDescription,
              timingDescription,
              `Ended ${ended}`,
              issue ? assessmentIssueText(issue) : null,
            ]
              .filter((value): value is string => value !== null)
              .join("; ");
            const metadataDescriptionId = `record-metadata-${record.id}`;
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
                  ref={(element) => {
                    if (element) recordButtons.current.set(record.id, element);
                    else recordButtons.current.delete(record.id);
                  }}
                  type="button"
                  className={styles.rowButton}
                  disabled={selectionMode && active}
                  aria-label={
                    selectionMode
                      ? `${checked ? "Deselect" : "Select"} ${record.method} ${target.label}`
                      : `${record.method} ${target.label}`
                  }
                  aria-pressed={selectionMode ? checked : undefined}
                  aria-describedby={metadataDescriptionId}
                  onClick={() => (selectionMode ? onToggle(record.id) : onSelect(record.id))}
                >
                  <span className={styles.method}>{record.method}</span>
                  <span className={styles.target} title={target.title}>
                    <strong className={styles.targetHost}>{target.host}</strong>
                    <span className={styles.targetPath}>{target.path}</span>
                  </span>
                  <span className={styles.status}>
                    <RecordStatus
                      status={record.status}
                      state={record.state}
                      assessment={record.assessment}
                    />
                  </span>
                  <span className={styles.metadata}>
                    <span className={styles.modelMetadata} title={modelDescription}>
                      {model} · {reasoningEffort}
                    </span>
                    <span className={styles.timing} title={timingDescription}>
                      {firstToken} / {totalDuration}
                    </span>
                    <span className={styles.timestamp} title={`Ended ${ended}`}>
                      {ended}
                    </span>
                    <span id={metadataDescriptionId} className="srOnly">
                      {metadataDescription}
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
                          ? `Cannot delete active ${record.method} ${target.label}`
                          : deletingRecordId === record.id
                            ? `Deleting ${record.method} ${target.label}`
                            : `Delete ${record.method} ${target.label}`
                      }
                      aria-busy={deletingRecordId === record.id}
                      title={active ? undefined : `Delete ${record.method} ${target.label}`}
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
        <button
          type="button"
          onClick={onPrevious}
          disabled={!hasPrevious || loading || deletionBusy}
        >
          <ChevronLeft size={15} aria-hidden="true" /> Previous
        </button>
        <span>
          Page {page} · {records.length} shown · {total} total
        </span>
        <button type="button" onClick={onNext} disabled={!hasNext || loading || deletionBusy}>
          Next <ChevronRight size={15} aria-hidden="true" />
        </button>
      </nav>
    </aside>
  );
}
