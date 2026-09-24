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
import type { AgentKind } from "@/domain/agent";
import { agentSelectionOptions, tenantSelectionOptions } from "@/features/common/tenantOptions";
import {
  readSessionRoute,
  sessionLocation,
  sessionTenantSelectionValue,
  tenantSelectionFromSessionValue,
  type SessionTab,
} from "@/features/sessions/route";
import {
  isConversationNotice,
  transcriptAttentionNotice,
  type SessionTimelineItem,
} from "@/features/sessions/detail/sessionDetail";
import {
  SESSION_AGENT_OPTIONS,
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
import type { TenantSelection, TenantSelectionValue } from "@/domain/tenant";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useSelectionModeFocus } from "@/features/common/useSelectionModeFocus";
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
    agent: AgentKind;
    agentOptions: SelectionOption<AgentKind>[];
    data: AggregatedSessionData | null;
    load: (kind?: "initial" | "refresh") => Promise<AggregatedSessionData | null>;
    loadingList: boolean;
    loadingTenants: boolean;
    refreshButton: RefObject<HTMLButtonElement | null>;
    refreshing: boolean;
    retryPageError: () => void;
    retryTenants: () => void;
    selectAgent: (values: ReadonlySet<AgentKind>) => void;
    selectTenant: (values: ReadonlySet<TenantSelectionValue>) => void;
    sessions: SourcedSession[];
    sessionTenantMissing: boolean;
    tenant: TenantSelection;
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
    /**
     * Re-reads the inspected Session in place, as Refresh does, and resolves
     * to the snapshot the new read reported — `null` if it did not complete.
     */
    refreshTranscript: () => Promise<string | null>;
    resolvedActiveUserMessage: string | null;
    sessionTab: SessionTab;
    sessionWarnings: string[];
    showJumpLatest: boolean;
    timeline: SessionTimelineItem[];
    transcriptHasDiagnostics: boolean;
    /** Why Conversation reading is impaired; `null` when the Transcript reads cleanly. */
    transcriptAttentionNotice: string | null;
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
  const { agent, sessionId, tab: sessionTab, tenant: rawTenant } = routeIntent;
  const selectedTenantKey = sessionTenantSelectionValue(rawTenant);
  const tenant = useMemo(
    () => tenantSelectionFromSessionValue(selectedTenantKey),
    [selectedTenantKey],
  );
  const routeSourceKey = `${selectedTenantKey}:${agent}`;
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
    setError(`Couldn’t load Session ${row.display_id}: ${messageOf(cause)}`);
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
    updateSessionLocation(sessionLocation(tenant, agent, currentSession?.id ?? sessionId, next));
  }
  useNarrowDetailFocus(detailHeadingRef, currentSession !== null, currentSession?.key);
  const tenantOptions = useMemo(() => tenantSelectionOptions(tenants), [tenants]);
  const agentOptions = useMemo(
    () => agentSelectionOptions(SESSION_AGENT_OPTIONS.map((option) => option.value)),
    [],
  );
  const sessionTenantMissing =
    !loadingTenants &&
    !tenantError &&
    selectedTenantKey.startsWith("managed:") &&
    !tenantOptions.some((option) => option.value === selectedTenantKey);
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
    tenant,
    agent,
  });
  const openSession = useCallback(
    async (row: SourcedSession, updateLocation = true, preserveContent = false) => {
      // A refresh keeps the reading where it is; only a new Session starts over.
      if (!preserveContent) clearConversation();
      setError(null);
      if (updateLocation) {
        updateSessionLocation(sessionLocation(tenant, agent, row.id, sessionTab));
      }
      await inspect(row, preserveContent);
    },
    [agent, clearConversation, inspect, sessionTab, tenant, updateSessionLocation],
  );
  const refreshTranscript = useCallback(async () => {
    const inspected = inspectedSession();
    if (!inspected) return null;
    const stats = await inspect(inspected, true);
    return stats?.snapshot ?? null;
  }, [inspect, inspectedSession]);
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
    tenant,
    agent,
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
    if (!sessionId) {
      if (inspectedSession()) clearInspection();
      return;
    }
    if (!data || loadingList) return;
    const row = data.sessions.find((candidate) => candidate.id === sessionId);
    if (row) {
      // URL-owned selection synchronizes the external detail stream lifecycle.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      if (inspectedSession()?.key !== row.key) void openSession(row, false);
      return;
    }
    // The refreshed catalog can invalidate a route-owned Session selection.
    clearInspection();
    updateSessionLocation(sessionLocation(tenant, agent), true);
  }, [
    agent,
    clearInspection,
    data,
    loadingList,
    openSession,
    sessionId,
    inspectedSession,
    tenant,
    updateSessionLocation,
  ]);
  const { enterSelection, cancelSelection } = useSelectionModeFocus({
    selectionMode,
    selectButton,
    fallbackButton: refreshButton,
    focusFirstSelectable: () => (data?.sessions ?? []).some((row) => sessionRows.focus(row.key)),
    onEnter: () => dispatchWorkflow({ type: "selection_enter" }),
    onExit: () => dispatchWorkflow({ type: "selection_cancel" }),
  });
  function toggleSession(key: string) {
    dispatchWorkflow({ type: "selection_toggle", key });
  }
  function toggleAllSessions() {
    const keys = data?.sessions.map((row) => row.key) ?? [];
    const allSelected = keys.length > 0 && keys.every((key) => selectedKeys.has(key));
    dispatchWorkflow({ type: "selection_toggle_all", keys, clear: allSelected });
  }
  function selectTenant(values: ReadonlySet<TenantSelectionValue>) {
    const value = [...values][0];
    if (!value) return;
    clearInspection();
    resetCatalog();
    updateSessionLocation(sessionLocation(tenantSelectionFromSessionValue(value), agent));
  }
  function selectAgent(values: ReadonlySet<AgentKind>) {
    const nextAgent = [...values][0];
    if (!nextAgent) return;
    clearInspection();
    resetCatalog();
    updateSessionLocation(sessionLocation(tenant, nextAgent));
  }
  function closeSessionInspection() {
    const focusKey = currentSession?.key ?? null;
    clearInspection();
    updateSessionLocation(sessionLocation(tenant, agent));
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
  const attentionNotice = transcriptAttentionNotice({
    partial: transcriptIsPartial,
    malformedCount: detailStats?.malformed_count ?? 0,
    listWarnings: sessionWarnings,
  });
  const userMessages = useMemo(
    () =>
      timeline.flatMap((item) =>
        item.kind === "message" && item.value.role === "user" && !isConversationNotice(item.value)
          ? [item.value]
          : [],
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
      agent,
      agentOptions,
      data,
      load,
      loadingList,
      loadingTenants,
      refreshButton,
      refreshing,
      retryPageError,
      retryTenants,
      selectAgent,
      selectTenant,
      sessions,
      sessionTenantMissing,
      tenant,
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
      refreshTranscript,
      registerUserMessage,
      resolvedActiveUserMessage,
      sessionTab,
      sessionWarnings,
      showJumpLatest,
      timeline,
      transcriptHasDiagnostics,
      transcriptAttentionNotice: attentionNotice,
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
      enterSelection,
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
