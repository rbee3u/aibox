import {
  Check,
  ChevronLeft,
  ChevronRight,
  Inbox,
  ListChecks,
  LoaderCircle,
  Trash2,
} from "lucide-react";
import { useEffect, useRef } from "react";
import { moduleIcons } from "@/shared/icons/consoleIcons";
import { elapsedNsMs, resolveRequestedEffective } from "@/features/requests/summary";
import type { RequestSummary } from "@/api/requests";
import { compactDuration, formatTimestamp } from "@/shared/lib/format";
import { requestUrl } from "@/features/requests/requestFormat";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/requests/catalog/RequestList.module.css";
import { RequestStatus } from "@/features/requests/RequestStatus";
import {
  assessmentIssueText,
  assessmentPresentation,
} from "@/features/requests/statusPresentation";
import { EmptyState } from "@/shared/ui/EmptyState";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { useElementRegistry } from "@/features/common/useElementRegistry";

const RequestIcon = moduleIcons.requests;

function PageTurnButton({
  direction,
  disabled,
  onClick,
}: {
  direction: "previous" | "next";
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" className={styles.pageTurn} onClick={onClick} disabled={disabled}>
      {direction === "previous" ? (
        <>
          <ChevronLeft size={15} aria-hidden="true" /> Previous
        </>
      ) : (
        <>
          Next <ChevronRight size={15} aria-hidden="true" />
        </>
      )}
    </button>
  );
}

function RequestPagination({
  className,
  page,
  totalPages,
  shown,
  total,
  hasPrevious,
  hasNext,
  locked,
  onPrevious,
  onNext,
}: {
  className: string;
  page: number;
  totalPages: number;
  shown: number;
  total: number;
  hasPrevious: boolean;
  hasNext: boolean;
  locked: boolean;
  onPrevious: () => void;
  onNext: () => void;
}) {
  return (
    <nav className={className} aria-label="Request pages">
      <PageTurnButton direction="previous" disabled={!hasPrevious || locked} onClick={onPrevious} />
      <span>
        Page {page} of {totalPages} · {shown} shown · {total} total
      </span>
      <PageTurnButton direction="next" disabled={!hasNext || locked} onClick={onNext} />
    </nav>
  );
}

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
  const deleteButtons = useElementRegistry<HTMLButtonElement>();
  const requestButtons = useElementRegistry<HTMLButtonElement>();

  useEffect(() => {
    if (focusAfterDelete === undefined) return;
    const preferred =
      focusAfterDelete === null ? refreshButton.current : deleteButtons.get(focusAfterDelete);
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterDelete();
  }, [deleteButtons, deletionBusy, focusAfterDelete, onFocusAfterDelete, requests]);

  useEffect(() => {
    if (focusAfterInspection === undefined) return;
    const preferred =
      focusAfterInspection === null
        ? refreshButton.current
        : requestButtons.get(focusAfterInspection);
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (!target || target.disabled) return;
    target.focus();
    onFocusAfterInspection();
  }, [focusAfterInspection, onFocusAfterInspection, requestButtons, requests]);

  useEffect(() => {
    if (selectionMode || !focusSelectAfterExit.current) return;
    focusSelectAfterExit.current = false;
    selectButton.current?.focus();
  }, [selectionMode]);

  const pageTurnLocked = loading || deletionBusy;
  const paginationProps = {
    page,
    totalPages,
    shown: requests.length,
    total,
    hasPrevious,
    hasNext,
    locked: pageTurnLocked,
    onPrevious,
    onNext,
  };

  return (
    <aside className={styles.panel} aria-label="Request list">
      <div className={styles.headerStack}>
        <div className={`${styles.listHeader} ${selectionMode ? layout.selectionBar : ""}`}>
          {selectionMode ? (
            <>
              <button
                type="button"
                className={layout.selectionCancel}
                onClick={() => {
                  focusSelectAfterExit.current = true;
                  onExitSelection();
                }}
              >
                Cancel
              </button>
              <div className={layout.selectionCenter}>
                <span className={styles.selectionCount}>{selected.size} selected</span>
                <button
                  type="button"
                  className={layout.selectionAll}
                  onClick={onTogglePage}
                  disabled={deletable.length === 0}
                >
                  {pageSelected ? "Clear page" : "Select page"}
                </button>
              </div>
              <button
                type="button"
                className={layout.selectionDelete}
                onClick={(event) => {
                  event.currentTarget.focus();
                  onDeleteSelected();
                }}
                disabled={selected.size === 0 || deletionBusy}
                aria-label="Delete selected"
                title="Delete selected"
              >
                <Trash2 size={14} aria-hidden="true" />
                Delete
              </button>
            </>
          ) : (
            <div className={styles.headerActions}>
              <RefreshButton
                ref={refreshButton}
                data-dialog-focus-fallback="true"
                onClick={onRefresh}
                disabled={refreshing || deletionBusy}
                label="Refresh Requests"
                busyLabel="Refreshing Requests"
                busy={refreshing}
              >
                Refresh
              </RefreshButton>
              <button
                ref={selectButton}
                type="button"
                className={layout.selectionEnter}
                aria-label="Select Requests"
                onClick={onEnterSelection}
                disabled={deletableCount === 0 || loading || deletionBusy}
              >
                <ListChecks size={14} aria-hidden="true" /> Select
              </button>
            </div>
          )}
        </div>
        {selectionMode && (
          <RequestPagination
            className={`${styles.pagination} ${styles.headerPagination}`}
            {...paginationProps}
          />
        )}
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
                    requestButtons.register(request.id, element);
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
                        deleteButtons.register(request.id, element);
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
      <RequestPagination className={styles.pagination} {...paginationProps} />
    </aside>
  );
}
