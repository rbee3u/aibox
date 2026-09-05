import type { RefObject, UIEvent } from "react";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";

import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type {
  ConversationMessage,
  SessionApi,
  SessionDetailMeta,
  SessionDetailStats,
} from "@/api/sessions";
import type { CodingAgentKind } from "@/domain/codingAgent";
import { agentSelectionOptions, tenantSelectionOptions } from "@/features/common/tenantOptions";
import { readSessionRoute, sessionLocation, type SessionTab } from "@/features/sessions/route";
import type { SessionDialogSource } from "@/features/sessions/sessionCatalog";
import {
  transcriptNeedsAttention,
  type SessionTimelineItem,
} from "@/features/sessions/detail/sessionDetail";
import {
  SESSION_AGENT_OPTIONS,
  sessionSource,
  visibleSessionSource,
  type AggregatedSessionData,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import { useSessionInspection } from "@/features/sessions/detail/useSessionInspection";
import {
  initialSessionWorkflow,
  sessionWorkflowReducer,
} from "@/features/sessions/sessionWorkflow";
import { useConversationNavigation } from "@/features/sessions/detail/useConversationNavigation";
import { useSessionCatalog } from "@/features/sessions/catalog/useSessionCatalog";
import {
  useSessionDeletion,
  type SessionDeletion,
} from "@/features/sessions/mutation/useSessionDeletion";
import type { TenantSelectionValue } from "@/domain/tenant";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";

interface ControllerOptions {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export interface SessionViewModel {
  catalog: {
    agentOptions: SelectionOption<CodingAgentKind>[];
    commitAgents: (values: ReadonlySet<CodingAgentKind>) => void;
    commitTenants: (values: ReadonlySet<TenantSelectionValue>) => void;
    data: AggregatedSessionData | null;
    load: (kind?: "initial" | "refresh") => Promise<AggregatedSessionData | null>;
    loadingList: boolean;
    loadingTenants: boolean;
    refreshButton: RefObject<HTMLButtonElement | null>;
    refreshing: boolean;
    retryPageError: () => void;
    retryTenants: () => void;
    selectedAgents: Set<CodingAgentKind>;
    selectedTenants: Set<TenantSelectionValue>;
    sessions: SourcedSession[];
    sessionTenantMissing: boolean;
    tenantError: string | null;
    tenantOptions: SelectionOption<TenantSelectionValue>[];
  };
  detail: {
    closeSessionInspection: () => void;
    conversationScrollRef: RefObject<HTMLDivElement | null>;
    currentSession: SourcedSession | null;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailMeta: SessionDetailMeta | null;
    detailRevision: number;
    detailStats: SessionDetailStats | null;
    jumpToLatest: () => void;
    jumpToUserMessage: (entryId: string) => void;
    loadingDetail: boolean;
    onConversationScroll: (event: UIEvent<HTMLDivElement>) => void;
    openSession: (
      row: SourcedSession,
      updateLocation?: boolean,
      preserveContent?: boolean,
    ) => Promise<void>;
    resolvedActiveUserMessage: string | null;
    sessionTab: SessionTab;
    sessionWarnings: string[];
    showJumpLatest: boolean;
    timeline: SessionTimelineItem[];
    transcriptHasDiagnostics: boolean;
    transcriptNeedsAttention: boolean;
    transcriptIsPartial: boolean;
    unsafeView: boolean;
    updateSessionTab: (next: SessionTab) => void;
    registerUserMessage: (entryId: string, element: HTMLElement | null) => void;
    userMessages: ConversationMessage[];
  };
  selection: {
    allSelected: boolean;
    cancelSelection: () => void;
    selectedKeys: Set<string>;
    selectionMode: boolean;
    registerSessionRow: (key: string, element: HTMLButtonElement | null) => void;
    enterSelection: () => void;
    toggleAllSessions: () => void;
    toggleSession: (key: string) => void;
  };
  mutations: {
    batchBusy: boolean;
    deleteSelectedSessions: () => Promise<void>;
    deleteSession: (row: SourcedSession) => Promise<void>;
    deletion: SessionDeletion;
    deletionBusy: boolean;
    mutationBusy: boolean;
  };
  dialogs: {
    registerDeleteButton: (key: string, element: HTMLButtonElement | null) => void;
    dialogKeys: string[] | null;
    dialogSources: SessionDialogSource[];
    selectButton: RefObject<HTMLButtonElement | null>;
    closeBatchDelete: () => void;
    closeSingleDelete: () => void;
    openBatchDelete: (keys: string[]) => void;
    openSingleDelete: (target: SourcedSession) => void;
    singleDeleteTarget: SourcedSession | null;
  };
  feedback: {
    dismissNotification: (source: NotificationSource) => void;
    error: string | null;
    notifications: NotificationItemData[];
  };
}

export function useSessionController({
  api,
  operation,
  search,
  onLocationChange,
}: ControllerOptions): SessionViewModel {
  const routeIntent = useMemo(() => readSessionRoute(search), [search]);
  const {
    agents: selectedAgents,
    selection: routeSelection,
    tab: sessionTab,
    tenants: selectedTenants,
  } = routeIntent;
  const routeSourceKey = JSON.stringify([[...selectedTenants].sort(), [...selectedAgents].sort()]);
  const previousSearch = useRef(search);
  const previousRouteSourceKey = useRef(routeSourceKey);
  const writtenSearch = useRef<string | null>(null);
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [workflow, dispatchWorkflow] = useReducer(sessionWorkflowReducer, initialSessionWorkflow);
  const { selectedKeys, selectionMode } = workflow;
  const [error, setError] = useState<string | null>(null);
  const reportInspectionFailure = useCallback((row: SourcedSession, cause: unknown) => {
    setError(`Couldn’t load Session from ${visibleSessionSource(row.source)}: ${messageOf(cause)}`);
  }, []);
  const inspection = useSessionInspection(api, reportInspectionFailure);
  const {
    timeline,
    meta: detailMeta,
    stats: detailStats,
    warnings: detailWarnings,
    loading: loadingDetail,
  } = inspection.detailState;
  const {
    abort: abortDetailStream,
    clear: clearDetailInspection,
    currentSession,
    detailRevision,
    inspect,
    inspectedSession,
    replaceCurrent,
  } = inspection;
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const {
    activeUserMessage,
    clear: clearConversation,
    conversationScrollRef,
    jumpToLatest,
    jumpToUserMessage,
    onConversationScroll,
    showJumpLatest,
    registerUserMessage,
  } = useConversationNavigation({
    active: sessionTab === "conversation",
    currentSessionKey: currentSession?.key,
    detailRevision,
    loading: loadingDetail,
  });
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const sessionRows = useElementRegistry<HTMLButtonElement>();
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();
  const updateSessionLocation = useCallback(
    (query: URLSearchParams, replace = false) => {
      const suffix = query.toString();
      writtenSearch.current = suffix ? `?${suffix}` : "";
      onLocationChange(query, replace);
    },
    [onLocationChange],
  );
  function updateSessionTab(next: SessionTab) {
    if (next === sessionTab) return;
    const selection = currentSession
      ? {
          tenantSelectionValue: currentSession.source.tenantSelectionValue,
          agent: currentSession.source.agent,
          id: currentSession.id,
        }
      : routeSelection;
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents, selection, next));
  }
  useNarrowDetailFocus(detailHeadingRef, currentSession !== null, currentSession?.key);
  const tenantOptions = useMemo(() => tenantSelectionOptions(tenants), [tenants]);
  const agentOptions = useMemo(
    () => agentSelectionOptions(SESSION_AGENT_OPTIONS.map((option) => option.value)),
    [],
  );
  const selectedSessionTenant = selectedTenants.size === 1 ? [...selectedTenants][0] : null;
  const sessionTenantMissing =
    !loadingTenants &&
    !tenantError &&
    selectedSessionTenant?.startsWith("managed:") === true &&
    !tenantOptions.some((option) => option.value === selectedSessionTenant);
  const tenantSourceKey = [...selectedTenants].sort().join(",");
  const agentSourceKey = SESSION_AGENT_OPTIONS.map((option) => option.value)
    .filter((agent) => selectedAgents.has(agent))
    .join(",");
  const sources = useMemo(() => {
    const tenantSelectionValues = tenantSourceKey
      .split(",")
      .filter((value): value is TenantSelectionValue => value.length > 0);
    const agents = agentSourceKey
      .split(",")
      .filter((value): value is CodingAgentKind => value === "codex" || value === "claude");
    return tenantSelectionValues.flatMap((tenantSelectionValue) =>
      agents.map((selectedAgent) => sessionSource(tenantSelectionValue, selectedAgent)),
    );
  }, [agentSourceKey, tenantSourceKey]);
  const clearInspection = useCallback(() => {
    clearDetailInspection();
    clearConversation();
  }, [clearConversation, clearDetailInspection]);
  const resetSourceLifecycle = useCallback(() => {
    setError(null);
    dispatchWorkflow({ type: "selection_cancel" });
  }, []);
  const resetSelection = useCallback(() => dispatchWorkflow({ type: "selection_cancel" }), []);
  const recoverSelection = useCallback(
    (remaining: Set<string>) =>
      dispatchWorkflow({ type: "selection_recovered", remaining, resume: true }),
    [],
  );
  const {
    data,
    load,
    loading: loadingList,
    refreshing,
    removeSession,
    reset: resetCatalog,
    unavailable: listUnavailable,
  } = useSessionCatalog({
    abortDetailStream,
    api,
    clearInspection,
    inspectedSession,
    onSelectionReset: resetSelection,
    onSourceLifecycleReset: resetSourceLifecycle,
    replaceCurrent,
    setError,
    sources,
  });
  const openSession = useCallback(
    async (row: SourcedSession, updateLocation = true, preserveContent = false) => {
      clearConversation();
      setError(null);
      if (updateLocation) {
        const nextSelection = {
          tenantSelectionValue: row.source.tenantSelectionValue,
          agent: row.source.agent,
          id: row.id,
        };
        updateSessionLocation(
          sessionLocation(selectedTenants, selectedAgents, nextSelection, sessionTab),
        );
      }
      await inspect(row, preserveContent);
    },
    [
      clearConversation,
      inspect,
      selectedAgents,
      selectedTenants,
      sessionTab,
      updateSessionLocation,
    ],
  );
  const sessionDeletion = useSessionDeletion({
    abortDetailStream,
    api,
    clearInspection,
    data,
    inspectedSession,
    listUnavailable,
    load,
    openSession,
    operation,
    onSelectionRecovery: recoverSelection,
    refreshButton,
    removeSession,
    reportFailure,
    resolveFailure,
    sourceKey: routeSourceKey,
  });
  useEffect(() => {
    const changed = previousSearch.current !== search;
    const sourceChanged = previousRouteSourceKey.current !== routeSourceKey;
    const locallyWritten = writtenSearch.current === search;
    previousSearch.current = search;
    previousRouteSourceKey.current = routeSourceKey;
    if (locallyWritten) writtenSearch.current = null;
    if (!changed || locallyWritten || sourceChanged) return;
    clearInspection();
    resetCatalog();
    void load();
  }, [clearInspection, load, resetCatalog, routeSourceKey, search]);
  useEffect(() => {
    if (!routeSelection) {
      if (inspectedSession()) clearInspection();
      return;
    }
    if (!data || loadingList) return;
    const row = data.sessions.find(
      (candidate) =>
        candidate.source.tenantSelectionValue === routeSelection.tenantSelectionValue &&
        candidate.source.agent === routeSelection.agent &&
        candidate.id === routeSelection.id,
    );
    if (row) {
      // URL-owned selection synchronizes the external detail stream lifecycle.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      if (inspectedSession()?.key !== row.key) void openSession(row, false);
      return;
    }
    // The refreshed catalog can invalidate a route-owned Session selection.
    clearInspection();
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents), true);
  }, [
    clearInspection,
    data,
    loadingList,
    openSession,
    routeSelection,
    inspectedSession,
    selectedAgents,
    selectedTenants,
    updateSessionLocation,
  ]);
  useEffect(() => {
    if (selectionMode || !focusSelectAfterExit.current) return;
    focusSelectAfterExit.current = false;
    const target = selectButton.current;
    if (target && !target.disabled) target.focus();
    else if (refreshButton.current && !refreshButton.current.disabled)
      refreshButton.current.focus();
  }, [selectionMode]);
  function toggleSession(key: string) {
    dispatchWorkflow({ type: "selection_toggle", key });
  }
  function toggleAllSessions() {
    const keys = data?.sessions.map((row) => row.key) ?? [];
    const allSelected = keys.length > 0 && keys.every((key) => selectedKeys.has(key));
    dispatchWorkflow({ type: "selection_toggle_all", keys, clear: allSelected });
  }
  function cancelSelection() {
    focusSelectAfterExit.current = true;
    dispatchWorkflow({ type: "selection_cancel" });
  }
  function commitTenants(values: ReadonlySet<TenantSelectionValue>) {
    const next = new Set(values);
    clearInspection();
    resetCatalog();
    updateSessionLocation(sessionLocation(next, selectedAgents));
  }
  function commitAgents(values: ReadonlySet<CodingAgentKind>) {
    const next = new Set(values);
    clearInspection();
    resetCatalog();
    updateSessionLocation(sessionLocation(selectedTenants, next));
  }
  function closeSessionInspection() {
    const focusKey = currentSession?.key ?? null;
    clearInspection();
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents));
    window.requestAnimationFrame(() => {
      if (focusKey) sessionRows.focus(focusKey);
    });
  }
  const unsafeView = listUnavailable || (data?.warnings.length ?? 0) > 0;
  const sessions = data?.sessions ?? [];
  const allSelected = sessions.length > 0 && sessions.every((row) => selectedKeys.has(row.key));
  const sessionWarnings = currentSession
    ? [...new Set([...currentSession.warnings, ...detailWarnings])]
    : [];
  const transcriptIsPartial = Boolean(currentSession && !loadingDetail && !detailStats);
  const transcriptHasDiagnostics =
    transcriptIsPartial ||
    sessionWarnings.length > 0 ||
    (detailStats?.malformed_count ?? 0) > 0 ||
    (detailStats?.unsupported_count ?? 0) > 0 ||
    (detailStats?.hidden_internal_count ?? 0) > 0;
  const transcriptNeedsAttentionFlag = transcriptNeedsAttention({
    partial: transcriptIsPartial,
    malformedCount: detailStats?.malformed_count ?? 0,
    listWarningCount: currentSession?.warnings.length ?? 0,
    timeline,
  });
  const userMessages = useMemo(
    () =>
      timeline.flatMap((item) =>
        item.kind === "message" && item.value.role === "user" ? [item.value] : [],
      ),
    [timeline],
  );
  const resolvedActiveUserMessage =
    activeUserMessage && userMessages.some((message) => message.entry_ids[0] === activeUserMessage)
      ? activeUserMessage
      : (userMessages[0]?.entry_ids[0] ?? null);
  function retryPageError() {
    setError(null);
    const inspected = inspectedSession();
    if (!listUnavailable && inspected) {
      void openSession(inspected, false);
    } else {
      void load("refresh");
    }
  }
  return {
    catalog: {
      agentOptions,
      commitAgents,
      commitTenants,
      data,
      load,
      loadingList,
      loadingTenants,
      refreshButton,
      refreshing,
      retryPageError,
      retryTenants,
      selectedAgents,
      selectedTenants,
      sessions,
      sessionTenantMissing,
      tenantError,
      tenantOptions,
    },
    detail: {
      closeSessionInspection,
      conversationScrollRef,
      currentSession,
      detailHeadingRef,
      detailMeta,
      detailRevision,
      detailStats,
      jumpToLatest,
      jumpToUserMessage,
      loadingDetail,
      onConversationScroll,
      openSession,
      registerUserMessage,
      resolvedActiveUserMessage,
      sessionTab,
      sessionWarnings,
      showJumpLatest,
      timeline,
      transcriptHasDiagnostics,
      transcriptNeedsAttention: transcriptNeedsAttentionFlag,
      transcriptIsPartial,
      unsafeView,
      updateSessionTab,
      userMessages,
    },
    selection: {
      allSelected,
      cancelSelection,
      selectedKeys,
      selectionMode,
      registerSessionRow: sessionRows.register,
      enterSelection: () => dispatchWorkflow({ type: "selection_enter" }),
      toggleAllSessions,
      toggleSession,
    },
    mutations: sessionDeletion.mutations,
    dialogs: { ...sessionDeletion.dialogs, selectButton },
    feedback: {
      dismissNotification,
      error,
      notifications,
    },
  };
}
