import { ArrowLeftRight, ChevronLeft, CircleAlert, LoaderCircle } from "lucide-react";

import type { RequestsApi } from "@/api/requests";
import { RequestDetail } from "@/features/requests/detail/RequestDetail";
import { RequestList } from "@/features/requests/catalog/RequestList";
import { REQUESTS_PER_PAGE } from "@/features/requests/catalog/listModel";
import { useRequestsController } from "@/features/requests/useRequestsController";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/requests/RequestsPage.module.css";

interface RequestsPageProps {
  api: RequestsApi;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export function RequestsPage(props: RequestsPageProps) {
  const {
    catalog: {
      currentId,
      list,
      loadingList,
      navigatePage,
      openRequest,
      page,
      refreshPage,
      refreshing,
    },
    detail: {
      bodies,
      bodyStatus,
      currentId: detailCurrentId,
      decodedBodies,
      detail: requestDetail,
      detailBackButton,
      detailOpen,
      download,
      eventTimings,
      inspectionFailure,
      loadingBody,
      loadingDetail,
      retryInspectionFailure,
      returnToList,
      selectTab,
      tab,
    },
    dialogs: { cancelDialog, confirmDelete, dialog },
    feedback: { dismissNotification, handleNotificationAction, notifications },
    mutations: { deletingRequestId, deletionBusy, openBatchDeletion, openRequestDeletion },
    selection: {
      clearFocusAfterDelete,
      clearFocusAfterInspection,
      enterSelection,
      exitSelection,
      focusAfterDelete,
      focusAfterInspection,
      selected,
      selectionMode,
      togglePageSelection,
      toggleRequestSelection,
    },
  } = useRequestsController(props);

  return (
    <div className={styles.app}>
      <div
        className={`${styles.main} ${detailOpen && currentId !== null ? styles.detailOpen : ""}`}
      >
        <div className={styles.listColumn}>
          <RequestList
            requests={list.requests}
            total={list.total}
            page={page}
            totalPages={Math.max(1, Math.ceil(list.total / REQUESTS_PER_PAGE))}
            hasPrevious={page > 1}
            hasNext={list.has_next}
            selectionMode={selectionMode}
            selected={selected}
            currentId={currentId}
            onEnterSelection={enterSelection}
            onExitSelection={exitSelection}
            onTogglePage={togglePageSelection}
            onToggle={toggleRequestSelection}
            onSelect={openRequest}
            onPrevious={() => navigatePage(page - 1)}
            onNext={() => navigatePage(page + 1)}
            loading={loadingList}
            refreshing={refreshing}
            deletableCount={list.deletable_count}
            onRefresh={() => void refreshPage()}
            onDeleteSelected={openBatchDeletion}
            onDeleteRequest={openRequestDeletion}
            deletingRequestId={deletingRequestId}
            deletionBusy={deletionBusy}
            focusAfterDelete={focusAfterDelete}
            onFocusAfterDelete={clearFocusAfterDelete}
            focusAfterInspection={focusAfterInspection}
            onFocusAfterInspection={clearFocusAfterInspection}
          />
        </div>
        <div className={styles.detailColumn}>
          {detailCurrentId && (
            <button
              ref={detailBackButton}
              type="button"
              className={styles.detailBack}
              aria-label="Back to Request list"
              title="Back to Request list"
              onClick={returnToList}
            >
              <ChevronLeft size={18} aria-hidden="true" />
            </button>
          )}
          {loadingDetail ? (
            <EmptyState
              className={styles.emptyDetail}
              variant="detail"
              icon={
                <LoaderCircle className={`${styles.loader} spin`} size={28} aria-label="Loading" />
              }
              description="Loading request…"
              role="status"
            />
          ) : requestDetail ? (
            <RequestDetail
              key={requestDetail.request.id}
              detail={requestDetail}
              bodies={bodies}
              bodyStatus={bodyStatus}
              decodedBodies={decodedBodies}
              eventTimings={eventTimings}
              tab={tab}
              onTabChange={selectTab}
              onDownload={(kind) => void download(kind)}
              loadingBody={loadingBody}
            />
          ) : detailCurrentId ? (
            <EmptyState
              className={styles.emptyDetail}
              variant="detail"
              icon={<CircleAlert size={26} aria-hidden="true" />}
              title="Request unavailable"
              description="Request details could not be loaded."
            >
              {inspectionFailure?.retryable !== false && (
                <RefreshButton type="button" label="Retry" onClick={retryInspectionFailure}>
                  Retry
                </RefreshButton>
              )}
            </EmptyState>
          ) : (
            <EmptyState
              className={styles.emptyDetail}
              variant="detail"
              icon={
                <ArrowLeftRight size={26} data-icon="request-detail-empty" aria-hidden="true" />
              }
              title="Select a Request"
              description="Choose a Request to inspect its summary and raw data."
            />
          )}
        </div>
      </div>
      <NotificationCenter
        notifications={notifications}
        paused={dialog !== null}
        onAction={handleNotificationAction}
        onDismiss={dismissNotification}
      />
      {dialog && (
        <ConfirmDialog
          title={
            dialog.kind === "request"
              ? "Delete this Request?"
              : `Delete ${dialog.ids.length} selected Request${dialog.ids.length === 1 ? "" : "s"}?`
          }
          message="Permanently deletes the selected raw Request and Response data."
          confirmLabel="Delete permanently"
          onConfirm={() => void confirmDelete()}
          onCancel={cancelDialog}
          busy={deletionBusy}
        />
      )}
    </div>
  );
}
