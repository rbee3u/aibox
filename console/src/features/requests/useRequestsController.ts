import { useCallback, useEffect, useReducer, useRef, useState, type RefObject } from "react";

import type { RequestList, RequestsApi } from "@/api/requests";
import { allSelected } from "@/features/common/catalogSelection";
import { requestWasCancelled } from "@/features/requests/requestErrors";
import {
  earliestSelectedPage,
  initialRequestsWorkflow,
  requestsWorkflowReducer,
  type RequestsDeletion,
  type RequestsDialog,
} from "@/features/requests/requestsWorkflow";
import { readRequestsRoute, requestsSearch, type RequestsRoute } from "@/features/requests/route";
import {
  useRequestInspection,
  type InspectionFailure,
} from "@/features/requests/detail/useRequestInspection";
import {
  focusTargetAfterDelete,
  removeDeletedFromList,
  REQUESTS_PER_PAGE,
} from "@/features/requests/catalog/listModel";
import type { DetailTab } from "@/features/requests/viewTypes";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import { usePolling } from "@/shared/hooks/usePolling";
import { LatestRequest } from "@/shared/lib/latestRequest";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";

type Inspection = ReturnType<typeof useRequestInspection>;

const LIST_POLL_INTERVAL_MS = 5000;

const emptyList: RequestList = {
  requests: [],
  total: 0,
  deletable_count: 0,
  has_next: false,
};

interface ControllerOptions {
  api: RequestsApi;
  search: string;
  onLocationChange: ModuleLocationChange;
}

/**
 * What the Requests page reads, grouped the same way the other three catalog
 * pages group their view models: the list, the open Request, batch selection,
 * deletion, its dialog, and page-level feedback.
 */
export interface RequestsViewModel {
  catalog: {
    currentId: string | null;
    list: RequestList;
    loadingList: boolean;
    navigatePage: (nextPage: number) => void;
    openRequest: (id: string) => void;
    page: number;
    refreshPage: () => Promise<void>;
    refreshing: boolean;
  };
  detail: {
    bodies: Inspection["bodies"];
    bodyStatus: Inspection["bodyStatus"];
    currentId: string | null;
    decodedBodies: Inspection["decodedBodies"];
    detail: Inspection["detail"];
    detailBackButton: RefObject<HTMLButtonElement | null>;
    detailOpen: boolean;
    download: Inspection["download"];
    eventTimings: Inspection["eventTimings"];
    inspectionFailure: Inspection["failure"];
    loadingBody: Inspection["loadingBody"];
    loadingDetail: boolean;
    retryInspectionFailure: () => void;
    returnToList: () => void;
    selectTab: (next: DetailTab) => void;
    tab: DetailTab;
  };
  selection: {
    clearFocusAfterDelete: () => void;
    clearFocusAfterInspection: () => void;
    enterSelection: () => void;
    exitSelection: () => void;
    focusAfterDelete: string | null | undefined;
    focusAfterInspection: string | null | undefined;
    selected: Set<string>;
    selectionMode: boolean;
    togglePageSelection: () => void;
    toggleRequestSelection: (id: string) => void;
  };
  mutations: {
    deletingRequestId: string | null;
    deletionBusy: boolean;
    openBatchDeletion: () => void;
    openRequestDeletion: (id: string) => void;
  };
  dialogs: {
    cancelDialog: () => void;
    confirmDelete: () => Promise<void>;
    dialog: RequestsDialog;
  };
  feedback: {
    dismissNotification: (source: NotificationSource) => void;
    handleNotificationAction: (notification: NotificationItemData) => void;
    notifications: NotificationItemData[];
  };
}

export function useRequestsController({
  api,
  search,
  onLocationChange,
}: ControllerOptions): RequestsViewModel {
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
  const [list, setList] = useState<RequestList>(emptyList);
  const [page, setPage] = useState(initialRoute.page);
  const pageRef = useRef(initialRoute.page);
  const [workflow, dispatchWorkflow] = useReducer(requestsWorkflowReducer, initialRequestsWorkflow);
  const { deletion, dialog, selectedKeys, selectionMode } = workflow;
  const [loadingList, setLoadingList] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [focusAfterInspection, setFocusAfterInspection] = useState<string | null | undefined>(
    undefined,
  );
  const [detailOpen, setDetailOpen] = useState(initialRoute.request !== null);
  const routeApplied = useRef(false);
  const detailBackButton = useRef<HTMLButtonElement>(null);
  const listRequest = useRef(new LatestRequest());
  const apiOwner = useRef(api);
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
    clearCurrentRequest,
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

  const openRequest = useCallback(
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
    clearCurrentRequest();
    updateLocation({ page: pageRef.current, request: null, tab: "summary" });
  }, [clearCurrentRequest, currentId, updateLocation]);

  const selectTab = useCallback(
    (next: DetailTab) => {
      if (next === tab) return;
      setTab(next);
      if (currentId) updateLocation({ page: pageRef.current, request: currentId, tab: next });
    },
    [currentId, setTab, tab, updateLocation],
  );

  function beginDeletion(next: Exclude<RequestsDeletion, null>): boolean {
    if (deletionInProgress.current) return false;
    deletionInProgress.current = true;
    listRequest.current.cancel();
    dispatchWorkflow({ type: "delete_started", deletion: next });
    return true;
  }

  function finishDeletion() {
    deletionInProgress.current = false;
    dispatchWorkflow({ type: "delete_finished" });
  }

  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RequestList | null> => {
      if (background && (pageNavigation.current || deletionInProgress.current)) return null;
      const targetPage = Math.max(1, pageToLoad);
      const request = listRequest.current.begin();
      if (!background) {
        pageNavigation.current = true;
        setLoadingList(true);
      }
      try {
        const payload = await api.listRequests(targetPage, request.signal);
        if (request.signal.aborted || !request.isCurrent()) return null;
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
        if (request.isCurrent() && !requestWasCancelled(cause, request.signal)) {
          if (!background || failedListPage.current === null) failedListPage.current = targetPage;
          reportFailure("list", "Couldn’t load requests", cause, true);
        }
        return null;
      } finally {
        if (request.isCurrent() && !background) {
          pageNavigation.current = false;
          setLoadingList(false);
        }
        request.release();
      }
    },
    [api, reportFailure, resolveFailure],
  );

  useEffect(() => {
    if (apiOwner.current === api) return;
    apiOwner.current = api;
    void loadPage(pageRef.current);
  }, [api, loadPage]);

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

  const cancelListRequest = useCallback(() => listRequest.current.cancel(), []);
  const pollList = useCallback(
    async (first: boolean) => {
      await refreshWithFallback(pageRef.current, !first);
    },
    [refreshWithFallback],
  );
  usePolling({
    enabled: !selectionMode && !dialogOpen,
    intervalMs: LIST_POLL_INTERVAL_MS,
    run: pollList,
    onCancel: cancelListRequest,
  });

  useEffect(() => {
    if (routeApplied.current) return;
    routeApplied.current = true;
    if (initialRoute.request) void selectRequest(initialRoute.request, initialRoute.tab);
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
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setDetailOpen(true);
      void selectRequest(route.request, route.tab);
    } else if (!route.request && currentId) {
      setDetailOpen(false);
      clearCurrentRequest();
    } else if (route.request && route.tab !== tab) {
      setTab(route.tab);
    }
  }, [
    clearCurrentRequest,
    currentId,
    loadPage,
    search,
    selectRequest,
    setTab,
    tab,
    updateLocation,
  ]);

  const deletableIdsOnPage = list.requests
    .filter((request) => request.state !== "active")
    .map((request) => request.id);

  async function confirmDelete() {
    if (!dialog) return;
    if (dialog.kind === "request") {
      await deleteRequest(dialog.id);
      return;
    }
    if (!beginDeletion({ kind: "batch" })) return;
    resolveFailure("action");
    const targetPage = earliestSelectedPage(workflow, dialog.ids, pageRef.current);
    try {
      const deletedCount = await api.deleteRequests(dialog.ids);
      const deletedIds = dialog.ids;
      setList((current) =>
        removeDeletedFromList(current, deletedIds, deletedCount, pageRef.current),
      );
      // Leaves selection mode whether or not every id was removed. Sessions
      // resumes a partial selection instead; keeping the two different is
      // deliberate rather than an oversight.
      dispatchWorkflow({ type: "selection_cancel" });
      if (currentId && deletedIds.includes(currentId)) {
        setDetailOpen(false);
        clearCurrentRequest();
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      }
      dispatchWorkflow({ type: "dialog_dismissed" });
      resolveFailure("action");
      await refreshWithFallback(targetPage);
      setFocusAfterDelete(null);
    } catch (cause) {
      const title =
        dialog.ids.length === 1 ? "Couldn’t delete request" : "Couldn’t delete requests";
      dispatchWorkflow({ type: "dialog_dismissed" });
      reportFailure("action", title, cause);
    } finally {
      finishDeletion();
    }
  }

  async function deleteRequest(id: string) {
    if (!beginDeletion({ kind: "request", id })) return;
    const originPage = pageRef.current;
    const originRequests = list.requests;
    resolveFailure("action");
    try {
      await api.deleteRequests([id]);
      if (currentId === id) {
        setDetailOpen(false);
        clearCurrentRequest();
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
      dispatchWorkflow({ type: "dialog_dismissed" });
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

  return {
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
      currentId,
      decodedBodies,
      detail,
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
    selection: {
      clearFocusAfterDelete: () => setFocusAfterDelete(undefined),
      clearFocusAfterInspection: () => setFocusAfterInspection(undefined),
      enterSelection: () => {
        setDetailOpen(false);
        dispatchWorkflow({ type: "selection_enter" });
      },
      exitSelection: () => dispatchWorkflow({ type: "selection_cancel" }),
      focusAfterDelete,
      focusAfterInspection,
      selected: selectedKeys,
      selectionMode,
      togglePageSelection: () =>
        dispatchWorkflow({
          type: "selection_toggle_all",
          keys: deletableIdsOnPage,
          clear: allSelected(deletableIdsOnPage, selectedKeys),
          context: pageRef.current,
        }),
      toggleRequestSelection: (id: string) =>
        dispatchWorkflow({ type: "selection_toggle", key: id, context: pageRef.current }),
    },
    mutations: {
      deletingRequestId,
      deletionBusy,
      openBatchDeletion: () =>
        dispatchWorkflow({
          type: "dialog_opened",
          dialog: { kind: "batch", ids: [...selectedKeys] },
        }),
      openRequestDeletion: (id: string) =>
        dispatchWorkflow({ type: "dialog_opened", dialog: { kind: "request", id } }),
    },
    dialogs: {
      cancelDialog: () => !deletionBusy && dispatchWorkflow({ type: "dialog_dismissed" }),
      confirmDelete,
      dialog,
    },
    feedback: {
      dismissNotification,
      handleNotificationAction,
      notifications: selectionMode
        ? notifications.map((notification) =>
            notification.source === "list"
              ? { ...notification, actionLabel: undefined }
              : notification,
          )
        : notifications,
    },
  };
}
