import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UIEvent } from "react";

import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type { SessionApi } from "@/api/sessions";
import type { CodingAgentKind } from "@/domain/codingAgent";
import {
  readSessionRoute,
  sessionLocation,
  type SessionRouteSelection,
  type SessionTab,
} from "@/features/sessions/route";
import {
  aggregateSessionCatalog,
  groupSessionsForDeletion,
  sessionDialogSources,
  splitSessionResults,
} from "@/features/sessions/sessionCatalog";
import { conversationIsAwayFromLatest } from "@/features/sessions/sessionFormat";
import {
  focusTargetAfterSessionDelete,
  SESSION_AGENT_OPTIONS,
  sessionSource,
  visibleSessionSource,
  type AggregatedSessionData,
  type SessionSource,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import { useSessionInspection } from "@/features/sessions/useSessionInspection";
import type { TenantSelectionKey } from "@/domain/tenant";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

interface ControllerOptions {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
}

type SessionDeletion = { kind: "record"; key: string } | { kind: "batch" } | null;

function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

export function useSessionController({
  api,
  operation,
  search,
  onLocationChange,
}: ControllerOptions) {
  const [initialRoute] = useState(() => readSessionRoute(search));
  const observedSearch = useRef(search);
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [selectedTenants, setSelectedTenants] = useState<Set<TenantSelectionKey>>(
    () => initialRoute.tenants,
  );
  const [selectedAgents, setSelectedAgents] = useState<Set<CodingAgentKind>>(
    () => initialRoute.agents,
  );
  const [routeSelection, setRouteSelection] = useState<SessionRouteSelection | null>(
    initialRoute.selection,
  );
  const [sessionTab, setSessionTab] = useState<SessionTab>(initialRoute.tab);
  const [data, setData] = useState<AggregatedSessionData | null>(null);
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [dialogKeys, setDialogKeys] = useState<string[] | null>(null);
  const [singleDeleteTarget, setSingleDeleteTarget] = useState<SourcedSession | null>(null);
  const [deletion, setDeletion] = useState<SessionDeletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [listUnavailable, setListUnavailable] = useState(false);
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const [activeUserMessage, setActiveUserMessage] = useState<string | null>(null);
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
  const conversationScrollRef = useRef<HTMLDivElement>(null);
  const userMessageRefs = useRef(new Map<string, HTMLElement>());
  const listRequest = useRef(new LatestRequest());
  const deletionInFlight = useRef(false);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const sessionRowButtons = useRef(new Map<string, HTMLButtonElement>());
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();
  const updateSessionLocation = useCallback(
    (query: URLSearchParams, replace = false) => {
      const suffix = query.toString();
      observedSearch.current = suffix ? `?${suffix}` : "";
      onLocationChange(query, replace);
    },
    [onLocationChange],
  );
  function updateSessionTab(next: SessionTab) {
    if (next === sessionTab) return;
    setSessionTab(next);
    const selection = currentSession
      ? {
          tenantKey: currentSession.source.tenantKey,
          agent: currentSession.source.agent,
          id: currentSession.id,
        }
      : routeSelection;
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents, selection, next));
  }
  function onConversationScroll(event: UIEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    setShowJumpLatest(conversationIsAwayFromLatest(element));
    const threshold = element.scrollTop + Math.min(element.clientHeight * 0.28, 180);
    let active: string | null = null;
    for (const [entryId, message] of userMessageRefs.current) {
      if (message.offsetTop <= threshold) active = entryId;
      else break;
    }
    if (active) setActiveUserMessage(active);
  }
  function jumpToLatest() {
    const element = conversationScrollRef.current;
    if (!element) return;
    if (typeof element.scrollTo === "function") {
      element.scrollTo({ top: element.scrollHeight, behavior: "smooth" });
    } else {
      element.scrollTop = element.scrollHeight;
    }
    setShowJumpLatest(false);
  }
  function jumpToUserMessage(entryId: string) {
    const container = conversationScrollRef.current;
    const message = userMessageRefs.current.get(entryId);
    if (!container || !message) return;
    const top = Math.max(0, message.offsetTop - 24);
    if (typeof container.scrollTo === "function") {
      container.scrollTo({ top, behavior: "smooth" });
    } else {
      container.scrollTop = top;
    }
    setActiveUserMessage(entryId);
  }
  useEffect(() => {
    if (!currentSession || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() => detailHeadingRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [currentSession]);
  const currentSessionKey = currentSession?.key;
  useEffect(() => {
    if (!currentSessionKey) return;
    const frame = window.requestAnimationFrame(() => {
      const element = conversationScrollRef.current;
      if (element && typeof element.scrollTo === "function") {
        element.scrollTo({ top: 0, behavior: "auto" });
      } else if (element) {
        element.scrollTop = 0;
      }
      setShowJumpLatest(false);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [currentSessionKey]);
  useEffect(() => {
    if (!currentSessionKey || sessionTab !== "conversation" || loadingDetail) return;
    const frame = window.requestAnimationFrame(() => {
      const element = conversationScrollRef.current;
      if (element) setShowJumpLatest(conversationIsAwayFromLatest(element));
    });
    return () => window.cancelAnimationFrame(frame);
  }, [currentSessionKey, detailRevision, loadingDetail, sessionTab]);
  const tenantOptions = useMemo<SelectionOption<TenantSelectionKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter(
        (
          tenant,
        ): tenant is TenantRow & {
          kind: "managed";
          name: string;
        } => Boolean(tenant.kind === "managed" && tenant.name),
      )
      .sort((left, right) => left.name.localeCompare(right.name));
    return [
      ...(host
        ? [
            {
              value: "host" as const,
              label: "Host Tenant",
              icon: <HostTenantIcon size={14} aria-hidden="true" />,
            },
          ]
        : []),
      ...managed.map((tenant) => ({
        value: `managed:${tenant.name}` as const,
        label: tenant.display_name,
        summaryLabel: tenant.display_name,
        icon: <ManagedTenantIcon size={14} aria-hidden="true" />,
      })),
    ];
  }, [tenants]);
  const agentOptions = useMemo<SelectionOption<CodingAgentKind>[]>(
    () =>
      SESSION_AGENT_OPTIONS.map((option) => ({
        ...option,
        icon: <BrandIcon brand={brandForAgent(option.value)} size={14} />,
      })),
    [],
  );
  const selectedSessionTenant = selectedTenants.size === 1 ? [...selectedTenants][0] : null;
  const sessionTenantMissing =
    !loadingTenants &&
    !tenantError &&
    selectedSessionTenant?.startsWith("managed:") === true &&
    !tenantOptions.some((option) => option.value === selectedSessionTenant);
  const sources = useMemo(() => {
    const tenantKeys = [...selectedTenants].sort();
    const agents = SESSION_AGENT_OPTIONS.map((option) => option.value).filter((agent) =>
      selectedAgents.has(agent),
    );
    return tenantKeys.flatMap((tenantKey) =>
      agents.map((selectedAgent) => sessionSource(tenantKey, selectedAgent)),
    );
  }, [selectedAgents, selectedTenants]);
  const clearInspection = useCallback(() => {
    clearDetailInspection();
    setActiveUserMessage(null);
    userMessageRefs.current.clear();
  }, [clearDetailInspection]);
  const openSession = useCallback(
    async (row: SourcedSession, updateLocation = true, preserveContent = false) => {
      setActiveUserMessage(null);
      userMessageRefs.current.clear();
      setError(null);
      if (updateLocation) {
        const nextSelection = {
          tenantKey: row.source.tenantKey,
          agent: row.source.agent,
          id: row.id,
        };
        setRouteSelection(nextSelection);
        updateSessionLocation(
          sessionLocation(selectedTenants, selectedAgents, nextSelection, sessionTab),
        );
      }
      await inspect(row, preserveContent);
    },
    [inspect, selectedAgents, selectedTenants, sessionTab, updateSessionLocation],
  );
  useEffect(() => {
    if (observedSearch.current === search) return;
    observedSearch.current = search;
    const route = readSessionRoute(search);
    clearInspection();
    setData(null);
    setSelectedTenants(route.tenants);
    setSelectedAgents(route.agents);
    setRouteSelection(route.selection);
    setSessionTab(route.tab);
  }, [clearInspection, search]);
  const load = useCallback(
    async (kind: "initial" | "refresh" = "initial"): Promise<AggregatedSessionData | null> => {
      const request = listRequest.current.begin();
      if (kind === "refresh") {
        setLoadingList(false);
        setRefreshing(true);
      } else {
        setRefreshing(false);
        setLoadingList(true);
      }
      try {
        const results = await Promise.allSettled(
          sources.map(async (source) => {
            const result = await api.listSessions(source.tenant, source.agent, request.signal);
            return { result, source };
          }),
        );
        if (request.signal.aborted || !request.isCurrent()) return null;
        const { successes, failures } = splitSessionResults(results, sources);
        if (successes.length === 0 && failures.length > 0) {
          const failureText = failures
            .map(({ cause, source }) => `${visibleSessionSource(source)}: ${messageOf(cause)}`)
            .join("; ");
          setListUnavailable(true);
          setError(`Couldn’t load Sessions: ${failureText}`);
          setData((current) =>
            kind === "refresh" && current ? current : { sessions: [], warnings: [], partial: true },
          );
          setSelectionMode(false);
          setSelectedKeys(new Set());
          return null;
        }
        const result: AggregatedSessionData = aggregateSessionCatalog(successes, failures);
        setData(result);
        setError(null);
        setListUnavailable(false);
        const inspected = inspectedSession();
        if (inspected) {
          const refreshed = result.sessions.find((row) => row.key === inspected.key);
          if (refreshed) {
            replaceCurrent(refreshed);
          } else {
            clearInspection();
          }
        }
        if (result.warnings.length > 0) {
          setSelectedKeys(new Set());
          setSelectionMode(false);
        }
        return result;
      } catch (cause) {
        if (request.isCurrent() && !sessionRequestCancelled(cause, request.signal)) {
          setError(messageOf(cause));
        }
        return null;
      } finally {
        if (request.isCurrent()) {
          if (kind === "refresh") setRefreshing(false);
          else setLoadingList(false);
        }
        request.release();
      }
    },
    [api, clearInspection, inspectedSession, replaceCurrent, sources],
  );
  useEffect(() => {
    const owner = listRequest.current;
    // A source-filter change starts a new external Session catalog lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    clearInspection();
    setData(null);
    setError(null);
    setListUnavailable(false);
    setSelectionMode(false);
    setSelectedKeys(new Set());
    setDialogKeys(null);
    setSingleDeleteTarget(null);
    setFocusAfterDelete(undefined);
    void load();
    return () => {
      owner.cancel();
      abortDetailStream();
    };
  }, [abortDetailStream, clearInspection, load]);
  useEffect(() => {
    if (!routeSelection || !data || loadingList) return;
    const row = data.sessions.find(
      (candidate) =>
        candidate.source.tenantKey === routeSelection.tenantKey &&
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
    setRouteSelection(null);
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
  useEffect(() => {
    if (focusAfterDelete === undefined || deletion !== null) return;
    const preferred = focusAfterDelete ? deleteButtons.current.get(focusAfterDelete) : null;
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (target && !target.disabled) {
      target.focus();
      setFocusAfterDelete(undefined);
    }
  }, [data, deletion, focusAfterDelete]);
  function toggleSession(key: string) {
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }
  function toggleAllSessions() {
    const keys = data?.sessions.map((row) => row.key) ?? [];
    const allSelected = keys.length > 0 && keys.every((key) => selectedKeys.has(key));
    setSelectedKeys(allSelected ? new Set() : new Set(keys));
  }
  function cancelSelection() {
    focusSelectAfterExit.current = true;
    setSelectionMode(false);
    setSelectedKeys(new Set());
  }
  function commitTenants(values: ReadonlySet<TenantSelectionKey>) {
    const next = new Set(values);
    clearInspection();
    setData(null);
    setRouteSelection(null);
    setSelectedTenants(next);
    updateSessionLocation(sessionLocation(next, selectedAgents));
  }
  function commitAgents(values: ReadonlySet<CodingAgentKind>) {
    const next = new Set(values);
    clearInspection();
    setData(null);
    setRouteSelection(null);
    setSelectedAgents(next);
    updateSessionLocation(sessionLocation(selectedTenants, next));
  }
  function closeSessionInspection() {
    const focusKey = currentSession?.key ?? null;
    clearInspection();
    setRouteSelection(null);
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents));
    window.requestAnimationFrame(() => {
      if (focusKey) sessionRowButtons.current.get(focusKey)?.focus();
    });
  }
  async function requestSessionDeletion(source: SessionSource, ids: string[]) {
    return api.deleteSessions(source.tenant, source.agent, ids);
  }
  function beginDeletion(next: Exclude<SessionDeletion, null>): boolean {
    if (deletionInFlight.current) return false;
    deletionInFlight.current = true;
    setDeletion(next);
    return true;
  }
  function finishDeletion() {
    deletionInFlight.current = false;
    setDeletion(null);
  }
  async function deleteSession(row: SourcedSession) {
    if (
      operation?.state === "running" ||
      data?.warnings.length ||
      listUnavailable ||
      !data ||
      !beginDeletion({ kind: "record", key: row.key })
    )
      return;
    const originRows = data.sessions;
    const wasCurrent = inspectedSession()?.key === row.key;
    if (wasCurrent) abortDetailStream();
    resolveFailure("action");
    try {
      await requestSessionDeletion(row.source, [row.id]);
      setData((current) =>
        current
          ? { ...current, sessions: current.sessions.filter((session) => session.key !== row.key) }
          : current,
      );
      if (wasCurrent) clearInspection();
      await load("refresh");
      setFocusAfterDelete(focusTargetAfterSessionDelete(originRows, row.key));
    } catch (cause) {
      reportFailure("action", "Couldn’t delete Session", cause);
      const refreshed = await load("refresh");
      const survivor = refreshed?.sessions.find((session) => session.key === row.key);
      if (wasCurrent && survivor) void openSession(survivor);
      setFocusAfterDelete(survivor ? row.key : null);
    } finally {
      setSingleDeleteTarget(null);
      finishDeletion();
    }
  }
  async function deleteSelectedSessions() {
    if (
      operation?.state === "running" ||
      !dialogKeys ||
      dialogKeys.length === 0 ||
      !beginDeletion({ kind: "batch" })
    )
      return;
    const keys = dialogKeys;
    const keySet = new Set(keys);
    const selectedRows = data?.sessions.filter((row) => keySet.has(row.key)) ?? [];
    const groups = groupSessionsForDeletion(selectedRows);
    const currentKey = inspectedSession()?.key;
    const wasCurrent = currentKey ? keySet.has(currentKey) : false;
    if (wasCurrent) clearInspection();
    resolveFailure("action");
    const failures: string[] = [];
    for (const { source, ids } of groups) {
      try {
        await requestSessionDeletion(source, ids);
      } catch (cause) {
        failures.push(`${visibleSessionSource(source)}: ${messageOf(cause)}`);
      }
    }
    setDialogKeys(null);
    if (failures.length > 0) {
      reportFailure(
        "action",
        "Couldn’t delete all selected Sessions",
        new Error(failures.join("; ")),
      );
    }
    const refreshed = await load("refresh");
    if (refreshed && refreshed.warnings.length === 0) {
      const remaining = new Set(
        keys.filter((key) => refreshed.sessions.some((row) => row.key === key)),
      );
      setSelectedKeys(remaining);
      setSelectionMode(remaining.size > 0);
      if (wasCurrent && currentKey) {
        const survivor = refreshed.sessions.find((row) => row.key === currentKey);
        if (survivor) void openSession(survivor);
      }
    }
    if (failures.length === 0) setFocusAfterDelete(null);
    finishDeletion();
  }
  const unsafeView = listUnavailable || (data?.warnings.length ?? 0) > 0;
  const sessions = data?.sessions ?? [];
  const allSelected = sessions.length > 0 && sessions.every((row) => selectedKeys.has(row.key));
  const deletionBusy = deletion !== null;
  const mutationBusy = deletionBusy || operation?.state === "running";
  const dialogSessions = dialogKeys
    ? sessions.filter((session) => dialogKeys.includes(session.key))
    : [];
  const dialogSources = sessionDialogSources(dialogSessions);
  const batchBusy = deletion?.kind === "batch";
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
    agentOptions,
    allSelected,
    batchBusy,
    cancelSelection,
    closeSessionInspection,
    commitAgents,
    commitTenants,
    conversationScrollRef,
    currentSession,
    data,
    deleteButtons,
    deleteSelectedSessions,
    deleteSession,
    deletion,
    deletionBusy,
    detailHeadingRef,
    detailMeta,
    detailRevision,
    detailStats,
    dialogKeys,
    dialogSources,
    dismissNotification,
    error,
    jumpToLatest,
    jumpToUserMessage,
    load,
    loadingDetail,
    loadingList,
    loadingTenants,
    mutationBusy,
    notifications,
    onConversationScroll,
    openSession,
    refreshButton,
    refreshing,
    resolvedActiveUserMessage,
    retryPageError,
    retryTenants,
    selectedAgents,
    selectedKeys,
    selectedTenants,
    selectionMode,
    selectButton,
    sessionRowButtons,
    sessions,
    sessionTab,
    sessionTenantMissing,
    sessionWarnings,
    setDialogKeys,
    setSelectionMode,
    setSingleDeleteTarget,
    showJumpLatest,
    singleDeleteTarget,
    tenantError,
    tenantOptions,
    timeline,
    toggleAllSessions,
    toggleSession,
    transcriptHasDiagnostics,
    transcriptIsPartial,
    unsafeView,
    updateSessionTab,
    userMessageRefs,
    userMessages,
  };
}
