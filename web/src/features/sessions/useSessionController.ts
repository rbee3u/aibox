import { useCallback, useMemo, useReducer, useRef, useState } from "react";

import type { TenantRow } from "@/api/core";

import type { AgentKind } from "@/domain/agent";
import {
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelectionValue,
} from "@/domain/tenant";
import {
  catalogSelectionReducer,
  initialCatalogSelection,
} from "@/features/common/catalogSelection";
import {
  readSessionRoute,
  sessionLocation,
  type SessionTab,
} from "@/features/common/routes/sessions";
import { agentSelectionOptions, tenantSelectionOptions } from "@/features/common/tenantOptions";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useSelectionModeFocus } from "@/features/common/useSelectionModeFocus";
import {
  isConversationNotice,
  transcriptAttentionNotice,
} from "@/features/sessions/detail/sessionDetail";
import { useConversationNavigation } from "@/features/sessions/detail/useConversationNavigation";
import { useSessionInspection } from "@/features/sessions/detail/useSessionInspection";
import { useSessionDeletion } from "@/features/sessions/mutation/useSessionDeletion";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import {
  useSessionRouteWriter,
  useSessionRouteInspection,
} from "@/features/sessions/useSessionRouteInspection";
import { useSessionScope } from "@/features/sessions/useSessionScope";
import type { SessionPageProps, SessionViewModel } from "@/features/sessions/viewTypes";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import { messageOf } from "@/shared/lib/errors";

export function useSessionController({
  api,
  operation,
  search,
  onLocationChange,
}: SessionPageProps): SessionViewModel {
  const routeIntent = useMemo(() => readSessionRoute(search), [search]);
  const { agent, sessionId, tab: sessionTab, tenant: rawTenant } = routeIntent;
  const selectedTenantKey = tenantSelectionValue(rawTenant);
  const tenant = useMemo(() => tenantSelectionFromValue(selectedTenantKey), [selectedTenantKey]);
  const routeSourceKey = `${selectedTenantKey}:${agent}`;
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [workflow, dispatchWorkflow] = useReducer(
    catalogSelectionReducer<string>,
    undefined,
    initialCatalogSelection<string>,
  );
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
  const { updateSessionLocation, consumeRouteChange } = useSessionRouteWriter(
    search,
    routeSourceKey,
    onLocationChange,
  );
  function updateSessionTab(next: SessionTab) {
    if (next === sessionTab) return;
    updateSessionLocation(sessionLocation(tenant, agent, currentSession?.id ?? sessionId, next));
  }
  useNarrowDetailFocus(detailHeadingRef, currentSession !== null, currentSession?.key);
  const tenantOptions = useMemo(() => tenantSelectionOptions(tenants), [tenants]);
  const agentOptions = useMemo(() => agentSelectionOptions(), []);
  const sessionTenantMissing =
    !loadingTenants &&
    !tenantError &&
    selectedTenantKey.startsWith("managed:") &&
    !tenantOptions.some((option) => option.value === selectedTenantKey);
  const clearInspection = useCallback(() => {
    clearDetailInspection();
    clearConversation();
  }, [clearConversation, clearDetailInspection]);
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
  } = useSessionScope({
    abortDetailStream,
    api,
    clearInspection,
    inspectedSession,
    resetSelection,
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
  useSessionRouteInspection({
    consumeRouteChange,
    routeSourceKey,
    search,
    clearInspection,
    resetCatalog,
    load,
    sessionId,
    data,
    loadingList,
    inspectedSession,
    openSession,
    tenant,
    agent,
    updateSessionLocation,
  });
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
    updateSessionLocation(sessionLocation(tenantSelectionFromValue(value), agent));
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
