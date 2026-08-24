import { ArrowLeftRight, ChevronLeft, CircleAlert, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { requestWasCancelled } from "./requestErrors";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { EmptyState } from "./components/EmptyState";
import { NotificationCenter, type NotificationItemData } from "./components/NotificationCenter";
import { RequestDetail } from "./components/RequestDetail";
import { RequestList } from "./components/RequestList";
import { useFailureNotifications } from "./useFailureNotifications";
import { useRequestInspection, type InspectionFailure } from "./useRequestInspection";
import type { RequestList as RequestListData, RequestSummary, RequestsApi } from "./types";
import type { DetailTab } from "./types";
import styles from "./RequestsPage.module.css";

interface RequestsPageProps {
  api: RequestsApi;
  search: string;
  onLocationChange: (query: URLSearchParams, replace?: boolean) => void;
}
type Dialog = { kind: "batch"; ids: string[] } | { kind: "request"; id: string } | null;
type Deletion = { kind: "batch" } | { kind: "request"; id: string } | null;

const LIST_POLL_INTERVAL_MS = 5000;
const REQUESTS_PER_PAGE = 50;
const DETAIL_TABS: readonly DetailTab[] = ["summary", "request", "response"];

interface RequestsLocation {
  page: number;
  request: string | null;
  tab: DetailTab;
}

export function parseRequestsLocation(search: string): RequestsLocation {
  const params = new URLSearchParams(search);
  const requestedPage = params.get("page");
  const parsedPage = requestedPage && /^\d+$/.test(requestedPage) ? Number(requestedPage) : 1;
  const page = Number.isSafeInteger(parsedPage) && parsedPage > 0 ? parsedPage : 1;
  const request = params.get("request")?.trim() || null;
  const requestedTab = params.get("tab");
  const tab =
    request && DETAIL_TABS.includes(requestedTab as DetailTab)
      ? (requestedTab as DetailTab)
      : "summary";
  return { page, request, tab };
}

export function serializeRequestsLocation(value: RequestsLocation): string {
  const params = new URLSearchParams();
  if (value.page > 1) params.set("page", String(value.page));
  if (value.request) params.set("request", value.request);
  if (value.request && value.tab !== "summary") params.set("tab", value.tab);
  const query = params.toString();
  return query ? `?${query}` : "";
}

export function RequestsPage({ api, search, onLocationChange }: RequestsPageProps) {
  const [initialLocation] = useState(() => parseRequestsLocation(search));
  const appliedSearch = useRef(serializeRequestsLocation(initialLocation));
  const updateLocation = useCallback(
    (value: RequestsLocation, replace = false) => {
      const next = serializeRequestsLocation(value);
      appliedSearch.current = next;
      onLocationChange(new URLSearchParams(next), replace);
    },
    [onLocationChange],
  );
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();
  const [list, setList] = useState<RequestListData>({
    requests: [],
    total: 0,
    deletable_count: 0,
    has_next: false,
  });
  const [page, setPage] = useState(initialLocation.page);
  const pageRef = useRef(initialLocation.page);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const selectionPages = useRef<Map<string, number>>(new Map());
  const [loadingList, setLoadingList] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deletion, setDeletion] = useState<Deletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [focusAfterInspection, setFocusAfterInspection] = useState<string | null | undefined>(
    undefined,
  );
  const [detailOpen, setDetailOpen] = useState(initialLocation.request !== null);
  const routeApplied = useRef(false);
  const detailBackButton = useRef<HTMLButtonElement>(null);
  const listController = useRef<AbortController | null>(null);
  const deletionInProgress = useRef(false);
  const pageNavigation = useRef(false);
  const failedListPage = useRef<number | null>(null);
  const initialLoadPending = useRef(true);
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
    initialTab: initialLocation.tab,
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

  useEffect(() => {
    if (!detailOpen || !currentId || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() => detailBackButton.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [currentId, detailOpen]);

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

  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelected(new Set());
    selectionPages.current.clear();
  }, []);
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

  useEffect(() => {
    if (selectionMode || dialogOpen) {
      listController.current?.abort();
      return;
    }
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await refreshWithFallback(pageRef.current, !initialLoadPending.current);
      initialLoadPending.current = false;
      if (!disposed) timer = window.setTimeout(() => void poll(), LIST_POLL_INTERVAL_MS);
    };
    void poll();
    return () => {
      disposed = true;
      listController.current?.abort();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [dialogOpen, refreshWithFallback, selectionMode]);

  useEffect(() => {
    if (routeApplied.current) return;
    routeApplied.current = true;
    const route = initialLocation;
    if (route.request) {
      void selectRequest(route.request, route.tab);
    }
  }, [initialLocation, selectRequest]);

  useEffect(() => {
    if (appliedSearch.current === search) return;
    appliedSearch.current = search;
    const route = parseRequestsLocation(search);
    const normalized = serializeRequestsLocation(route);
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

  const selectedIds = [...selected];

  function toggleRecordSelection(id: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.delete(id)) {
        selectionPages.current.delete(id);
      } else {
        next.add(id);
        selectionPages.current.set(id, pageRef.current);
      }
      return next;
    });
  }

  function togglePageSelection() {
    setSelected((current) => {
      const next = new Set(current);
      const deletableIds = list.requests
        .filter((request) => request.state !== "active")
        .map((request) => request.id);
      const pageSelected = deletableIds.length > 0 && deletableIds.every((id) => current.has(id));
      deletableIds.forEach((id) => {
        if (pageSelected) {
          next.delete(id);
          selectionPages.current.delete(id);
        } else if (!next.has(id)) {
          next.add(id);
          selectionPages.current.set(id, pageRef.current);
        }
      });
      return next;
    });
  }

  async function confirmDelete() {
    if (!dialog) return;
    if (dialog.kind === "request") {
      await deleteRecord(dialog.id);
      return;
    }
    if (!beginDeletion({ kind: "batch" })) return;
    resolveFailure("action");
    const targetPage = dialog.ids.reduce(
      (minimum, id) => Math.min(minimum, selectionPages.current.get(id) ?? pageRef.current),
      Number.MAX_SAFE_INTEGER,
    );
    try {
      const deletedCount = await api.deleteRequests(dialog.ids);
      const deletedIds = dialog.ids;
      setList((current) =>
        removeDeletedFromList(current, deletedIds, deletedCount, pageRef.current),
      );
      setSelected(new Set());
      selectionPages.current.clear();
      setSelectionMode(false);
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
            selectionMode={selectionMode}
            selected={selected}
            currentId={currentId}
            onEnterSelection={() => {
              setDetailOpen(false);
              setSelectionMode(true);
            }}
            onExitSelection={exitSelectionMode}
            onTogglePage={togglePageSelection}
            onToggle={toggleRecordSelection}
            onSelect={openRecord}
            onPrevious={() => navigatePage(page - 1)}
            onNext={() => navigatePage(page + 1)}
            loading={loadingList}
            refreshing={refreshing}
            deletableCount={list.deletable_count}
            onRefresh={() => void refreshPage()}
            onDeleteSelected={() => setDialog({ kind: "batch", ids: selectedIds })}
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
                <button type="button" onClick={retryInspectionFailure}>
                  Retry
                </button>
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
          selectionMode
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

function focusTargetAfterDelete(
  before: RequestSummary[],
  deletedId: string,
  after: RequestSummary[],
  movedToPreviousPage: boolean,
): string | null {
  const deletableAfter = after.filter((request) => request.state !== "active");
  if (deletableAfter.length === 0) return null;
  if (movedToPreviousPage) return deletableAfter.at(-1)?.id ?? null;

  const deletedIndex = before.findIndex((request) => request.id === deletedId);
  if (deletedIndex >= 0) {
    const adjacentIds = [
      ...before.slice(deletedIndex + 1),
      ...before.slice(0, deletedIndex).reverse(),
    ]
      .filter((request) => request.state !== "active")
      .map((request) => request.id);
    const remainingIds = new Set(deletableAfter.map((request) => request.id));
    const adjacentId = adjacentIds.find((id) => remainingIds.has(id));
    if (adjacentId) return adjacentId;
  }

  const start = Math.min(Math.max(deletedIndex, 0), after.length - 1);
  return (
    after.slice(start).find((request) => request.state !== "active")?.id ??
    after
      .slice(0, start)
      .reverse()
      .find((request) => request.state !== "active")?.id ??
    null
  );
}

function removeDeletedFromList(
  current: RequestListData,
  ids: readonly string[],
  deletedCount: number,
  currentPage: number,
): RequestListData {
  const deleted = new Set(ids);
  const total = Math.max(0, current.total - deletedCount);
  return {
    ...current,
    requests: current.requests.filter((request) => !deleted.has(request.id)),
    total,
    deletable_count: Math.max(0, current.deletable_count - deletedCount),
    has_next: currentPage * REQUESTS_PER_PAGE < total,
  };
}
