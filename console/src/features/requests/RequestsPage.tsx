import { ArrowLeftRight, ChevronLeft, CircleAlert, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { RequestList as RequestListData, RequestsApi } from "@/api/requests";
import { RequestDetail } from "@/features/requests/components/RequestDetail";
import { RequestList } from "@/features/requests/components/RequestList";
import { requestWasCancelled } from "@/features/requests/requestErrors";
import { readRequestsRoute, requestsSearch, type RequestsRoute } from "@/features/requests/route";
import {
  useRequestInspection,
  type InspectionFailure,
} from "@/features/requests/useRequestInspection";
import {
  focusTargetAfterDelete,
  removeDeletedFromList,
  REQUESTS_PER_PAGE,
} from "@/features/requests/requestList";
import type { DetailTab } from "@/features/requests/viewTypes";
import { useCatalogSelection } from "@/shared/hooks/useCatalogSelection";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import { usePolling } from "@/shared/hooks/usePolling";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import type { NotificationItemData } from "@/shared/ui/notificationTypes";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/requests/RequestsPage.module.css";

interface RequestsPageProps {
  api: RequestsApi;
  search: string;
  onLocationChange: ModuleLocationChange;
}
type Dialog = { kind: "batch"; ids: string[] } | { kind: "request"; id: string } | null;
type Deletion = { kind: "batch" } | { kind: "request"; id: string } | null;

const LIST_POLL_INTERVAL_MS = 5000;

const emptyList: RequestListData = {
  requests: [],
  total: 0,
  deletable_count: 0,
  has_next: false,
};

export function RequestsPage({ api, search, onLocationChange }: RequestsPageProps) {
  const [initialRoute] = useState(() => readRequestsRoute(search));
  const appliedSearch = useRef(requestsSearch(initialRoute));
  const updateLocation = useCallback(
    (value: RequestsRoute, replace = false) => {
      const next = requestsSearch(value);
      appliedSearch.current = next;
      onLocationChange(new URLSearchParams(next), replace);
    },
    [onLocationChange],
  );
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();
  const [list, setList] = useState<RequestListData>(emptyList);
  const [page, setPage] = useState(initialRoute.page);
  const pageRef = useRef(initialRoute.page);
  const selection = useCatalogSelection<number>();
  const [loadingList, setLoadingList] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deletion, setDeletion] = useState<Deletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [focusAfterInspection, setFocusAfterInspection] = useState<string | null | undefined>(
    undefined,
  );
  const [detailOpen, setDetailOpen] = useState(initialRoute.request !== null);
  const routeApplied = useRef(false);
  const detailBackButton = useRef<HTMLButtonElement>(null);
  const listController = useRef<AbortController | null>(null);
  const deletionInProgress = useRef(false);
  const pageNavigation = useRef(false);
  const failedListPage = useRef<number | null>(null);
  const deletingRequestId = deletion?.kind === "request" ? deletion.id : null;
  const deletionBusy = deletion !== null;
  const dialogOpen = dialog !== null;

  const handleInspectionFailure = useCallback(
    (failure: InspectionFailure) => {
      const title =
        failure.kind === "detail"
          ? "Couldn’t load request"
          : failure.kind === "body"
            ? "Couldn’t load Body"
            : "Couldn’t download Body";
      reportFailure("inspection", title, failure.message, failure.retryable !== false);
      if (failure.kind === "detail" && failure.retryable === false) {
        setDetailOpen(false);
        setFocusAfterInspection(null);
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      }
    },
    [reportFailure, updateLocation],
  );
  const handleInspectionRecovery = useCallback(
    () => resolveFailure("inspection"),
    [resolveFailure],
  );
  const inspection = useRequestInspection({
    api,
    initialTab: initialRoute.tab,
    paused: dialogOpen,
    onFailure: handleInspectionFailure,
    onRecovery: handleInspectionRecovery,
  });
  const {
    bodies,
    bodyStatus,
    clearCurrentRecord,
    clearRequestIfCurrent,
    currentId,
    decodedBodies,
    detail,
    download,
    eventTimings,
    failure: inspectionFailure,
    loadingBody,
    loadingDetail,
    retryFailure: retryInspectionFailure,
    selectRequest,
    setTab,
    tab,
  } = inspection;
  const currentIdRef = useRef(currentId);
  const tabRef = useRef(tab);
  useEffect(() => {
    currentIdRef.current = currentId;
    tabRef.current = tab;
  }, [currentId, tab]);

  useNarrowDetailFocus(detailBackButton, detailOpen && currentId !== null, currentId);

  const openRecord = useCallback(
    (id: string) => {
      setFocusAfterInspection(undefined);
      setDetailOpen(true);
      updateLocation({ page: pageRef.current, request: id, tab: "summary" });
      void selectRequest(id);
    },
    [selectRequest, updateLocation],
  );

  const returnToList = useCallback(() => {
    setFocusAfterInspection(currentId);
    setDetailOpen(false);
    clearCurrentRecord();
    updateLocation({ page: pageRef.current, request: null, tab: "summary" });
  }, [clearCurrentRecord, currentId, updateLocation]);

  const selectTab = useCallback(
    (next: DetailTab) => {
      if (next === tab) return;
      setTab(next);
      if (currentId) updateLocation({ page: pageRef.current, request: currentId, tab: next });
    },
    [currentId, setTab, tab, updateLocation],
  );

  function beginDeletion(next: Exclude<Deletion, null>): boolean {
    if (deletionInProgress.current) return false;
    deletionInProgress.current = true;
    listController.current?.abort();
    setDeletion(next);
    return true;
  }

  function finishDeletion() {
    deletionInProgress.current = false;
    setDeletion(null);
  }

  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RequestListData | null> => {
      if (background && (pageNavigation.current || deletionInProgress.current)) return null;
      const targetPage = Math.max(1, pageToLoad);
      listController.current?.abort();
      const controller = new AbortController();
      listController.current = controller;
      if (!background) {
        pageNavigation.current = true;
        setLoadingList(true);
      }
      try {
        const payload = await api.listRequests(targetPage, controller.signal);
        if (listController.current !== controller || controller.signal.aborted) return null;
        setList(payload);
        setPage(targetPage);
        pageRef.current = targetPage;
        if (
          !background ||
          failedListPage.current === null ||
          failedListPage.current === targetPage
        ) {
          failedListPage.current = null;
          resolveFailure("list");
        }
        return payload;
      } catch (cause) {
        if (
          listController.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          if (!background || failedListPage.current === null) failedListPage.current = targetPage;
          reportFailure("list", "Couldn’t load requests", cause, true);
        }
        return null;
      } finally {
        if (listController.current === controller) {
          listController.current = null;
          if (!background) {
            pageNavigation.current = false;
            setLoadingList(false);
          }
        }
      }
    },
    [api, reportFailure, resolveFailure],
  );

  const navigatePage = useCallback(
    (nextPage: number) => {
      const target = Math.max(1, nextPage);
      updateLocation({ page: target, request: currentId, tab });
      void loadPage(target).then((payload) => {
        if (payload || failedListPage.current !== target) return;
        updateLocation({ page: pageRef.current, request: currentId, tab }, true);
      });
    },
    [currentId, loadPage, tab, updateLocation],
  );

  /**
   * Loads `targetPage`, then walks toward page 1 while the loaded page is empty,
   * so deleting the last rows of a page never leaves the Console on a blank one.
   */
  const refreshWithFallback = useCallback(
    async (targetPage = pageRef.current, background = false) => {
      let candidate = Math.max(1, targetPage);
      while (true) {
        const payload = await loadPage(candidate, background);
        if (!payload) return null;
        if (payload.requests.length > 0 || candidate === 1) return { page: candidate, payload };
        const lastPage = Math.max(1, Math.ceil(payload.total / REQUESTS_PER_PAGE));
        candidate = Math.min(candidate - 1, lastPage);
        updateLocation(
          { page: candidate, request: currentIdRef.current, tab: tabRef.current },
          true,
        );
      }
    },
    [loadPage, updateLocation],
  );

  const refreshPage = useCallback(async () => {
    setRefreshing(true);
    try {
      await refreshWithFallback(page);
    } finally {
      setRefreshing(false);
    }
  }, [page, refreshWithFallback]);

  const retryListFailure = useCallback(async () => {
    const targetPage = failedListPage.current ?? pageRef.current;
    setRefreshing(true);
    try {
      const refreshed = await refreshWithFallback(targetPage);
      if (!refreshed) return;
      updateLocation(
        { page: refreshed.page, request: currentIdRef.current, tab: tabRef.current },
        true,
      );
    } finally {
      setRefreshing(false);
    }
  }, [refreshWithFallback, updateLocation]);

  const cancelListRequest = useCallback(() => listController.current?.abort(), []);
  const pollList = useCallback(
    async (first: boolean) => {
      await refreshWithFallback(pageRef.current, !first);
    },
    [refreshWithFallback],
  );
  usePolling({
    enabled: !selection.active && !dialogOpen,
    intervalMs: LIST_POLL_INTERVAL_MS,
    run: pollList,
    onCancel: cancelListRequest,
  });

  useEffect(() => {
    if (routeApplied.current) return;
    routeApplied.current = true;
    if (initialRoute.request) {
      void selectRequest(initialRoute.request, initialRoute.tab);
    }
  }, [initialRoute, selectRequest]);

  useEffect(() => {
    if (appliedSearch.current === search) return;
    appliedSearch.current = search;
    const route = readRequestsRoute(search);
    const normalized = requestsSearch(route);
    if (normalized !== search) {
      updateLocation(route, true);
      return;
    }
    if (route.page !== pageRef.current) void loadPage(route.page);
    if (route.request && route.request !== currentId) {
      // The App-owned history snapshot is an external navigation input for this page.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setDetailOpen(true);
      void selectRequest(route.request, route.tab);
    } else if (!route.request && currentId) {
      setDetailOpen(false);
      clearCurrentRecord();
    } else if (route.request && route.tab !== tab) {
      setTab(route.tab);
    }
  }, [clearCurrentRecord, currentId, loadPage, search, selectRequest, setTab, tab, updateLocation]);

  const deletableIdsOnPage = list.requests
    .filter((request) => request.state !== "active")
    .map((request) => request.id);

  async function confirmDelete() {
    if (!dialog) return;
    if (dialog.kind === "request") {
      await deleteRecord(dialog.id);
      return;
    }
    if (!beginDeletion({ kind: "batch" })) return;
    resolveFailure("action");
    const targetPage = dialog.ids.reduce(
      (minimum, id) => Math.min(minimum, selection.contextOf(id) ?? pageRef.current),
      Number.MAX_SAFE_INTEGER,
    );
    try {
      const deletedCount = await api.deleteRequests(dialog.ids);
      const deletedIds = dialog.ids;
      setList((current) =>
        removeDeletedFromList(current, deletedIds, deletedCount, pageRef.current),
      );
      selection.exit();
      if (currentId && deletedIds.includes(currentId)) {
        setDetailOpen(false);
        clearCurrentRecord();
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      }
      setDialog(null);
      resolveFailure("action");
      await refreshWithFallback(targetPage);
      setFocusAfterDelete(null);
    } catch (cause) {
      const title =
        dialog.ids.length === 1 ? "Couldn’t delete request" : "Couldn’t delete requests";
      setDialog(null);
      reportFailure("action", title, cause);
    } finally {
      finishDeletion();
    }
  }

  async function deleteRecord(id: string) {
    if (!beginDeletion({ kind: "request", id })) return;
    const originPage = pageRef.current;
    const originRequests = list.requests;
    resolveFailure("action");
    try {
      await api.deleteRequests([id]);
      if (currentId === id) {
        setDetailOpen(false);
        clearCurrentRecord();
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      } else {
        clearRequestIfCurrent(id);
      }
      setList((current) => removeDeletedFromList(current, [id], 1, pageRef.current));
      setFocusAfterDelete(
        focusTargetAfterDelete(
          originRequests,
          id,
          originRequests.filter((request) => request.id !== id),
          false,
        ),
      );
      const refreshed = await refreshWithFallback(originPage);
      if (refreshed) {
        setFocusAfterDelete(
          focusTargetAfterDelete(
            originRequests,
            id,
            refreshed.payload.requests,
            refreshed.page !== originPage,
          ),
        );
      }
    } catch (cause) {
      reportFailure("action", "Couldn’t delete request", cause);
    } finally {
      setDialog(null);
      finishDeletion();
    }
  }

  function handleNotificationAction(notification: NotificationItemData) {
    resolveFailure(notification.source);
    if (notification.source === "list") {
      void retryListFailure();
    } else if (notification.source === "inspection") {
      retryInspectionFailure();
    }
  }

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
            selectionMode={selection.active}
            selected={selection.selected}
            currentId={currentId}
            onEnterSelection={() => {
              setDetailOpen(false);
              selection.enter();
            }}
            onExitSelection={selection.exit}
            onTogglePage={() => selection.toggleAll(deletableIdsOnPage, pageRef.current)}
            onToggle={(id) => selection.toggle(id, pageRef.current)}
            onSelect={openRecord}
            onPrevious={() => navigatePage(page - 1)}
            onNext={() => navigatePage(page + 1)}
            loading={loadingList}
            refreshing={refreshing}
            deletableCount={list.deletable_count}
            onRefresh={() => void refreshPage()}
            onDeleteSelected={() => setDialog({ kind: "batch", ids: selection.ids })}
            onDeleteRequest={(id) => setDialog({ kind: "request", id })}
            deletingRequestId={deletingRequestId}
            deletionBusy={deletionBusy}
            focusAfterDelete={focusAfterDelete}
            onFocusAfterDelete={() => setFocusAfterDelete(undefined)}
            focusAfterInspection={focusAfterInspection}
            onFocusAfterInspection={() => setFocusAfterInspection(undefined)}
          />
        </div>
        <div className={styles.detailColumn}>
          {currentId && (
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
          ) : detail ? (
            <RequestDetail
              key={detail.request.id}
              detail={detail}
              bodies={bodies}
              bodyStatus={bodyStatus}
              decodedBodies={decodedBodies}
              eventTimings={eventTimings}
              tab={tab}
              onTabChange={selectTab}
              onDownload={(kind) => void download(kind)}
              loadingBody={loadingBody}
            />
          ) : currentId ? (
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
        notifications={
          selection.active
            ? notifications.map((notification) =>
                notification.source === "list"
                  ? { ...notification, actionLabel: undefined }
                  : notification,
              )
            : notifications
        }
        paused={dialogOpen}
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
          message="This permanently deletes the selected raw Request and Response data."
          confirmLabel="Delete permanently"
          onConfirm={() => void confirmDelete()}
          onCancel={() => !deletionBusy && setDialog(null)}
          busy={deletionBusy}
        />
      )}
    </div>
  );
}
