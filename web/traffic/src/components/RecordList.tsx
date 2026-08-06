import { ChevronLeft, ChevronRight, Clock3, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import type { RecordSummary } from "../types";
import { duration, recordUrl } from "../utils";
import styles from "./RecordList.module.css";
import { RecordStatus } from "./RecordStatus";

interface RecordListProps {
  records: RecordSummary[];
  total: number;
  page: number;
  hasPrevious: boolean;
  hasNext: boolean;
  selected: Set<string>;
  currentId: string | null;
  onSelectPage: (checked: boolean) => void;
  onToggle: (id: string, checked: boolean) => void;
  onSelect: (id: string) => void;
  onPrevious: () => void;
  onNext: () => void;
  loading: boolean;
  refreshing: boolean;
  deletableCount: number;
  onRefresh: () => void;
  onDeleteSelected: () => void;
  onDeleteAll: () => void;
}

export function RecordList({
  records,
  total,
  page,
  hasPrevious,
  hasNext,
  selected,
  currentId,
  onSelectPage,
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
}: RecordListProps) {
  const deletable = records.filter((record) => record.state !== "active");
  const selectedOnPage = deletable.filter((record) => selected.has(record.id)).length;
  const pageSelected = deletable.length > 0 && selectedOnPage === deletable.length;
  const pagePartiallySelected = selectedOnPage > 0 && !pageSelected;
  const selectPageCheckbox = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (selectPageCheckbox.current)
      selectPageCheckbox.current.indeterminate = pagePartiallySelected;
  }, [pagePartiallySelected]);

  return (
    <aside className={styles.panel} aria-label="Traffic records">
      <div className={styles.listHeader}>
        <h2>Traffic records</h2>
        <div className={styles.headerActions}>
          <button
            type="button"
            className={styles.refreshButton}
            onClick={onRefresh}
            disabled={refreshing}
            aria-label={refreshing ? "Refreshing traffic records" : "Refresh traffic records"}
            aria-busy={refreshing}
            title="Refresh traffic records"
          >
            <RefreshCw
              className={refreshing ? styles.refreshing : undefined}
              size={15}
              aria-hidden="true"
            />
          </button>
          <button
            type="button"
            className={styles.deleteAll}
            onClick={onDeleteAll}
            disabled={deletableCount === 0}
          >
            <Trash2 size={14} aria-hidden="true" /> Delete all
          </button>
        </div>
      </div>
      <div className={styles.tools}>
        <label className={styles.selectPage}>
          <input
            ref={selectPageCheckbox}
            type="checkbox"
            checked={pageSelected}
            disabled={deletable.length === 0}
            aria-checked={pagePartiallySelected ? "mixed" : pageSelected}
            onChange={(event) => onSelectPage(event.target.checked)}
          />
          Select page
        </label>
        {selected.size > 0 ? (
          <div className={styles.selectionActions}>
            <span>{selected.size} selected</span>
            <button
              type="button"
              className={styles.deleteSelected}
              onClick={onDeleteSelected}
              aria-label="Delete selected"
              title="Delete selected"
            >
              <Trash2 size={14} aria-hidden="true" />
              <span className={styles.deleteSelectedLabel}>Delete selected</span>
            </button>
          </div>
        ) : (
          <span>Page {page + 1}</span>
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
            return (
              <div
                key={record.id}
                className={`${styles.record} ${currentId === record.id ? styles.current : ""}`}
              >
                <input
                  type="checkbox"
                  disabled={active}
                  checked={selected.has(record.id)}
                  aria-label={`Select ${record.method} ${host}`}
                  onChange={(event) => onToggle(record.id, event.target.checked)}
                />
                <button
                  type="button"
                  className={styles.rowButton}
                  onClick={() => onSelect(record.id)}
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
                    <span>{new Date(record.started_at).toLocaleTimeString()}</span>
                    <span>{duration(record.total_ms)}</span>
                  </span>
                </button>
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
          {total} record{total === 1 ? "" : "s"}
        </span>
        <button type="button" onClick={onNext} disabled={!hasNext || loading}>
          Next <ChevronRight size={15} aria-hidden="true" />
        </button>
      </nav>
    </aside>
  );
}
