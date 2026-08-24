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
import { moduleIcons } from "../consoleIcons";
import { elapsedNsMs, resolveRequestedEffective } from "../summary";
import type { RequestSummary } from "../types";
import { compactDuration, formatTimestamp, requestUrl } from "../utils";
import styles from "./RequestList.module.css";
import { RequestStatus } from "./RequestStatus";
import { assessmentIssueText, assessmentPresentation } from "./statusPresentation";
import { EmptyState } from "./EmptyState";
import { IconButton } from "./IconButton";

const RequestIcon = moduleIcons.requests;

interface RequestListProps {
  requests: RequestSummary[];
  total: number;
  page: number;
  totalPages: number;
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
  onDeleteRequest: (id: string) => void;
  deletingRequestId: string | null;
  deletionBusy: boolean;
  focusAfterDelete: string | null | undefined;
  onFocusAfterDelete: () => void;
  focusAfterInspection: string | null | undefined;
  onFocusAfterInspection: () => void;
}

export function RequestList({
  requests,
  total,
  page,
  totalPages,
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
  onDeleteRequest,
  deletingRequestId,
  deletionBusy,
  focusAfterDelete,
  onFocusAfterDelete,
  focusAfterInspection,
  onFocusAfterInspection,
}: RequestListProps) {
  const deletable = requests.filter((request) => request.state !== "active");
  const selectedOnPage = deletable.filter((request) => selected.has(request.id)).length;
  const pageSelected = deletable.length > 0 && selectedOnPage === deletable.length;
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const requestButtons = useRef(new Map<string, HTMLButtonElement>());

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
  }, [deletionBusy, focusAfterDelete, onFocusAfterDelete, requests]);

  useEffect(() => {
    if (focusAfterInspection === undefined) return;
    const preferred =
      focusAfterInspection === null
        ? refreshButton.current
        : requestButtons.current.get(focusAfterInspection);
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterInspection();
  }, [focusAfterInspection, onFocusAfterInspection, requests]);

  useEffect(() => {
    if (selectionMode || !focusSelectAfterExit.current) return;
    focusSelectAfterExit.current = false;
    selectButton.current?.focus();
  }, [selectionMode]);

  return (
    <aside className={styles.panel} aria-label="Request list">
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
            <IconButton
              buttonRef={refreshButton}
              data-dialog-focus-fallback="true"
              className={styles.refreshButton}
              onClick={onRefresh}
              disabled={refreshing || deletionBusy}
              label={refreshing ? "Refreshing Request list" : "Refresh Request list"}
              aria-busy={refreshing}
            >
              <RefreshCw className={refreshing ? "spin" : undefined} size={14} aria-hidden="true" />
            </IconButton>
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
              className={styles.selectRequests}
              onClick={onEnterSelection}
              disabled={deletableCount === 0 || loading || deletionBusy}
            >
              <ListChecks size={14} aria-hidden="true" /> Select
            </button>
          )}
        </div>
      </div>
      <div className={styles.requests} aria-busy={loading}>
        {loading && requests.length === 0 ? (
          <div className={styles.loadingState} role="status" aria-live="polite">
            <LoaderCircle className="spin" size={22} aria-hidden="true" />
            <p>Loading Requests…</p>
          </div>
        ) : requests.length === 0 ? (
          <EmptyState
            variant="list"
            icon={<Inbox size={22} data-icon="request-empty" aria-hidden="true" />}
            title="No request recorded yet."
          />
        ) : (
          requests.map((request) => {
            const target = requestUrl(request);
            const active = request.state === "active";
            const checked = selected.has(request.id);
            const model = resolveRequestedEffective(request.protocol?.model) ?? "—";
            const reasoningEffort =
              resolveRequestedEffective(request.protocol?.reasoning_effort) ?? "—";
            const compactModel = reasoningEffort === "—" ? model : `${model} ${reasoningEffort}`;
            const firstToken = compactDuration(elapsedNsMs(request.protocol?.first_token_at_ns));
            const totalDuration = compactDuration(request.total_ms);
            const timestampKind = request.ended_at ? "Ended" : "Started";
            const timestampValue = request.ended_at ?? request.started_at;
            const timestamp = formatTimestamp(timestampValue);
            const issue = assessmentPresentation(request.assessment);
            const modelDescription = `Model ${model}; Reasoning effort ${reasoningEffort}`;
            const timingDescription = `First token ${firstToken}; Duration ${totalDuration}`;
            const metadataDescription = [
              modelDescription,
              timingDescription,
              `${timestampKind} ${timestamp}`,
              issue ? assessmentIssueText(issue) : null,
            ]
              .filter((value): value is string => value !== null)
              .join("; ");
            const metadataDescriptionId = `request-metadata-${request.id}`;
            return (
              <div
                key={request.id}
                className={[
                  styles.request,
                  currentId === request.id ? styles.current : "",
                  selectionMode ? styles.selectionRecord : "",
                  checked ? styles.selected : "",
                  selectionMode && active ? styles.unselectable : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <button
                  ref={(element) => {
                    if (element) requestButtons.current.set(request.id, element);
                    else requestButtons.current.delete(request.id);
                  }}
                  type="button"
                  className={styles.rowButton}
                  disabled={selectionMode && active}
                  aria-label={
                    selectionMode
                      ? `${checked ? "Deselect" : "Select"} ${request.method} ${target.label}`
                      : `${request.method} ${target.label}`
                  }
                  aria-pressed={selectionMode ? checked : undefined}
                  aria-describedby={metadataDescriptionId}
                  onClick={() => (selectionMode ? onToggle(request.id) : onSelect(request.id))}
                >
                  <RequestIcon
                    className={styles.requestIcon}
                    size={16}
                    data-icon="request-row"
                    aria-hidden="true"
                  />
                  <span className={styles.method}>{request.method}</span>
                  <span className={styles.target} title={target.title}>
                    <strong className={styles.targetHost}>{target.host}</strong>
                    <span className={styles.targetPath}>{target.path}</span>
                  </span>
                  <span className={styles.status}>
                    <RequestStatus
                      status={request.status}
                      state={request.state}
                      assessment={request.assessment}
                    />
                  </span>
                  <span className={styles.metadata}>
                    <span className={styles.modelMetadata} title={modelDescription}>
                      {compactModel}
                    </span>
                    <span className={styles.timingMetadata}>
                      <span className={styles.timing} title={timingDescription}>
                        {firstToken} / {totalDuration}
                      </span>
                      <time
                        className={styles.timestamp}
                        dateTime={timestampValue}
                        title={`${timestampKind} ${timestamp}`}
                      >
                        {timestamp}
                      </time>
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
                    title={active ? "Active requests cannot be deleted" : undefined}
                  >
                    <button
                      ref={(element) => {
                        if (element) deleteButtons.current.set(request.id, element);
                        else deleteButtons.current.delete(request.id);
                      }}
                      type="button"
                      className={styles.deleteRecord}
                      onClick={() => onDeleteRequest(request.id)}
                      disabled={active || deletionBusy}
                      aria-label={
                        active
                          ? `Cannot delete active ${request.method} ${target.label}`
                          : deletingRequestId === request.id
                            ? `Deleting ${request.method} ${target.label}`
                            : `Delete ${request.method} ${target.label}`
                      }
                      aria-busy={deletingRequestId === request.id}
                      title={active ? undefined : `Delete ${request.method} ${target.label}`}
                    >
                      {deletingRequestId === request.id ? (
                        <LoaderCircle className="spin" size={15} aria-hidden="true" />
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
      <nav className={styles.pagination} aria-label="Request pages">
        <button
          type="button"
          onClick={onPrevious}
          disabled={!hasPrevious || loading || deletionBusy}
        >
          <ChevronLeft size={15} aria-hidden="true" /> Previous
        </button>
        <span>
          Page {page} of {totalPages} · {requests.length} shown · {total} total
        </span>
        <button type="button" onClick={onNext} disabled={!hasNext || loading || deletionBusy}>
          Next <ChevronRight size={15} aria-hidden="true" />
        </button>
      </nav>
    </aside>
  );
}
