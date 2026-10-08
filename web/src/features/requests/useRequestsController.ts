import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { allSelected } from "@/features/common/catalogSelection";
import { useRequestCatalog } from "@/features/requests/catalog/useRequestCatalog";
import { useRequestInspection } from "@/features/requests/detail/useRequestInspection";
import { useRequestDeletion } from "@/features/requests/mutation/useRequestDeletion";
import {
  initialRequestsWorkflow,
  requestsWorkflowReducer,
} from "@/features/requests/requestsWorkflow";
import { readRequestsRoute, requestsSearch, type RequestsRoute } from "@/features/requests/route";
import type {
  RequestsPageProps,
  InspectionFailure,
  RequestsViewModel,
  DetailTab,
} from "@/features/requests/viewTypes";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";

import type { NotificationItemData } from "@/shared/ui/notificationTypes";

export function useRequestsController({
  api,
  search,
  onLocationChange,
}: RequestsPageProps): RequestsViewModel {
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
  const pageRef = useRef(initialRoute.page);
  const [workflow, dispatchWorkflow] = useReducer(requestsWorkflowReducer, initialRequestsWorkflow);
  const { deletion, dialog, selectedKeys, selectionMode } = workflow;
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [focusAfterInspection, setFocusAfterInspection] = useState<string | null | undefined>(
    undefined,
  );
  const [detailOpen, setDetailOpen] = useState(initialRoute.request !== null);
  const routeApplied = useRef(false);
  const detailBackButton = useRef<HTMLButtonElement>(null);
  const deletionInProgress = useRef(false);
  const deletingRequestId = deletion?.kind === "request" ? deletion.id : null;
  const deletionBusy = deletion !== null;
  const dialogOpen = dialog !== null;

  const handleInspectionFailure = useCallback(
    (failure: InspectionFailure) => {
      // Retryable detail errors already appear in the pane; closing it needs a notice.
      const dismissesDetail = failure.kind === "detail" && failure.retryable === false;
      if (failure.kind !== "detail" || dismissesDetail) {
        const title =
          failure.kind === "detail"
            ? "Couldn’t load request"
            : failure.kind === "body"
              ? "Couldn’t load Body"
              : "Couldn’t download Body";
        reportFailure("inspection", title, failure.message, failure.retryable !== false);
      }
      if (dismissesDetail) {
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

  const onFallbackPage = useCallback(
    (candidate: number) =>
      updateLocation({ page: candidate, request: currentIdRef.current, tab: tabRef.current }, true),
    [updateLocation],
  );
  const {
    list,
    setList,
    page,
    loadingList,
    refreshing,
    listError,
    loadPage,
    failedListPage,
    refreshWithFallback,
    refreshPage,
    retryListFailure,
    cancelListRequest,
  } = useRequestCatalog({
    api,
    initialPage: initialRoute.page,
    pageRef,
    deletionInProgress,
    enabled: !selectionMode && !dialogOpen,
    onFallbackPage,
  });
  const navigatePage = useCallback(
    (nextPage: number) => {
      const target = Math.max(1, nextPage);
      updateLocation({ page: target, request: currentId, tab });
      void loadPage(target).then((payload) => {
        if (payload || failedListPage.current !== target) return;
        updateLocation({ page: pageRef.current, request: currentId, tab }, true);
      });
    },
    [currentId, failedListPage, loadPage, tab, updateLocation],
  );

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

  const { confirmDelete } = useRequestDeletion({
    api,
    workflow,
    dispatchWorkflow,
    deletionInProgress,
    cancelListRequest,
    list,
    setList,
    currentId,
    setDetailOpen,
    clearCurrentRequest,
    clearRequestIfCurrent,
    updateLocation,
    pageRef,
    refreshWithFallback,
    setFocusAfterDelete,
    resolveFailure,
    reportFailure,
  });
  function handleNotificationAction(notification: NotificationItemData) {
    resolveFailure(notification.source);
    if (notification.source === "inspection") retryInspectionFailure();
  }

  return {
    catalog: {
      currentId,
      list,
      listError,
      loadingList,
      navigatePage,
      openRequest,
      page,
      refreshPage,
      refreshing,
      retryList: selectionMode ? undefined : () => void retryListFailure(),
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
      notifications,
    },
  };
}
