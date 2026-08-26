import { AlertTriangle, Box, ChevronLeft, ListChecks, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { UIEvent } from "react";
import type { CodingAgentKind, TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type { SessionApi, SessionDetailMeta, SessionDetailStats } from "@/api/sessions";
import { SessionConversation } from "@/features/sessions/components/SessionConversation";
import { SessionRow } from "@/features/sessions/components/SessionRow";
import { SessionDetails } from "@/features/sessions/components/SessionDetails";
import {
  readSessionRoute,
  sessionLocation,
  type SessionRouteSelection,
  type SessionTab,
} from "@/features/sessions/route";
import {
  appendActivityItem,
  appendConversationMessage,
  emptySessionDetail,
  sessionDetailReducer,
  type SessionActivityItem,
  type SessionTimelineItem,
} from "@/features/sessions/sessionDetail";
import {
  conversationIsAwayFromLatest,
  messageCountLabel,
  toolCountLabel,
} from "@/features/sessions/sessionFormat";
import {
  compareSessions,
  focusTargetAfterSessionDelete,
  SESSION_AGENT_OPTIONS,
  sessionSource,
  sourcedSession,
  visibleSessionListSource,
  visibleSessionSource,
  type AggregatedSessionData,
  type SessionSource,
  type SessionTenantKey,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";
import { useTenants } from "@/shared/hooks/useTenants";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { messageOf } from "@/shared/lib/errors";
import { compactDuration, formatTimestamp } from "@/shared/lib/format";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu, type SelectionOption } from "@/shared/ui/SelectionMenu";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

interface PageProps {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange?: ModuleLocationChange;
}

type SessionDeletion = { kind: "record"; key: string } | { kind: "batch" } | null;

/**
 * Sessions aggregates several Tenant-and-Agent scopes into one list. A request
 * cancelled by this page reports itself through its own signal or an
 * `AbortError` DOMException.
 */
function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

export function SessionPage({ api, operation, search, onLocationChange }: PageProps) {
  const [initialRoute] = useState(() => readSessionRoute(search));
  const observedSearch = useRef(search);
  const {
    tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useTenants(api);
  const [selectedTenants, setSelectedTenants] = useState<Set<SessionTenantKey>>(
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
  const [currentSession, setCurrentSession] = useState<SourcedSession | null>(null);
  const [detailState, dispatchDetail] = useReducer(sessionDetailReducer, emptySessionDetail);
  const {
    timeline,
    meta: detailMeta,
    stats: detailStats,
    warnings: detailWarnings,
    loading: loadingDetail,
  } = detailState;
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
  const [detailRevision, setDetailRevision] = useState(0);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const conversationScrollRef = useRef<HTMLDivElement>(null);
  const userMessageRefs = useRef(new Map<string, HTMLElement>());
  const listController = useRef<AbortController | null>(null);
  const streamController = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SourcedSession | null>(null);
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
      onLocationChange?.(query, replace);
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
  const tenantOptions = useMemo<SelectionOption<SessionTenantKey>[]>(() => {
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
  const abortDetailStream = useCallback(() => {
    streamController.current?.abort();
    streamController.current = null;
    dispatchDetail({ type: "stop" });
  }, []);
  const clearInspection = useCallback(() => {
    abortDetailStream();
    currentSessionRef.current = null;
    setCurrentSession(null);
    dispatchDetail({ type: "reset" });
    setActiveUserMessage(null);
    userMessageRefs.current.clear();
  }, [abortDetailStream]);
  const openSession = useCallback(
    async (row: SourcedSession, updateLocation = true, preserveContent = false) => {
      abortDetailStream();
      const controller = new AbortController();
      streamController.current = controller;
      currentSessionRef.current = row;
      setCurrentSession(row);
      setDetailRevision((current) => current + 1);
      setActiveUserMessage(null);
      userMessageRefs.current.clear();
      dispatchDetail({ type: "start", preserveContent });
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
      let nextTimeline: SessionTimelineItem[] = [];
      let nextMeta: SessionDetailMeta | null = null;
      let nextStats: SessionDetailStats | null = null;
      let nextWarnings: string[] = [];
      try {
        await api.streamSessionDetail(
          row.source.tenant,
          row.source.agent,
          row.id,
          {
            onMeta: (meta) => {
              if (preserveContent) nextMeta = meta;
              else dispatchDetail({ type: "meta", value: meta });
            },
            onMessage: (message) => {
              if (preserveContent) nextTimeline = appendConversationMessage(nextTimeline, message);
              else dispatchDetail({ type: "message", value: message });
            },
            onTool: (tool) => {
              const entry: SessionActivityItem = { kind: "tool", value: tool };
              if (preserveContent) nextTimeline = appendActivityItem(nextTimeline, entry);
              else dispatchDetail({ type: "activity", value: entry });
            },
            onEvidence: (evidence) => {
              const entry: SessionActivityItem = { kind: "evidence", value: evidence };
              if (preserveContent) nextTimeline = appendActivityItem(nextTimeline, entry);
              else dispatchDetail({ type: "activity", value: entry });
            },
            onComplete: (stats, warnings) => {
              if (preserveContent) {
                nextStats = stats;
                nextWarnings = warnings;
              } else {
                dispatchDetail({ type: "complete", stats, warnings });
              }
            },
          },
          controller.signal,
        );
        if (preserveContent && streamController.current === controller) {
          dispatchDetail({
            type: "replace",
            timeline: nextTimeline,
            meta: nextMeta,
            stats: nextStats,
            warnings: nextWarnings,
          });
        }
      } catch (cause) {
        if (!sessionRequestCancelled(cause, controller.signal)) {
          setError(
            `Couldn’t load Session from ${visibleSessionSource(row.source)}: ${messageOf(cause)}`,
          );
        }
      } finally {
        if (streamController.current === controller) {
          streamController.current = null;
          dispatchDetail({ type: "stop" });
        }
      }
    },
    [abortDetailStream, api, selectedAgents, selectedTenants, sessionTab, updateSessionLocation],
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
      listController.current?.abort();
      const controller = new AbortController();
      listController.current = controller;
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
            const result = await api.listSessions(source.tenant, source.agent, controller.signal);
            return { result, source };
          }),
        );
        if (listController.current !== controller || controller.signal.aborted) return null;
        const failures = results.flatMap((result, index) =>
          result.status === "rejected"
            ? [{ cause: result.reason as unknown, source: sources[index] }]
            : [],
        );
        const successes = results.flatMap((result) =>
          result.status === "fulfilled" ? [result.value] : [],
        );
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
        const warnings = [
          ...failures.map(
            ({ cause, source }) => `${visibleSessionSource(source)}: ${messageOf(cause)}`,
          ),
          ...successes.flatMap(({ result, source }) =>
            result.warnings.map((warning) => `${visibleSessionSource(source)}: ${warning}`),
          ),
        ];
        const sessions = successes
          .flatMap(({ result, source }) =>
            result.sessions.map((row) => sourcedSession(source, row)),
          )
          .sort(compareSessions);
        const result: AggregatedSessionData = {
          sessions,
          warnings,
          partial: failures.length > 0 || successes.some(({ result: value }) => value.partial),
        };
        setData(result);
        setError(null);
        setListUnavailable(false);
        const inspected = currentSessionRef.current;
        if (inspected) {
          const refreshed = result.sessions.find((row) => row.key === inspected.key);
          if (refreshed) {
            currentSessionRef.current = refreshed;
            setCurrentSession(refreshed);
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
        if (!sessionRequestCancelled(cause, controller.signal)) setError(messageOf(cause));
        return null;
      } finally {
        if (listController.current === controller) {
          listController.current = null;
          if (kind === "refresh") setRefreshing(false);
          else setLoadingList(false);
        }
      }
    },
    [api, clearInspection, sources],
  );
  useEffect(() => {
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
      listController.current?.abort();
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
      if (currentSessionRef.current?.key !== row.key) void openSession(row, false);
      return;
    }
    // The refreshed catalog can invalidate a route-owned Session selection.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setRouteSelection(null);
    clearInspection();
    updateSessionLocation(sessionLocation(selectedTenants, selectedAgents), true);
  }, [
    clearInspection,
    data,
    loadingList,
    openSession,
    routeSelection,
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
  function commitTenants(values: ReadonlySet<SessionTenantKey>) {
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
    const wasCurrent = currentSessionRef.current?.key === row.key;
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
    const groups = new Map<
      string,
      {
        source: SessionSource;
        ids: string[];
      }
    >();
    for (const row of selectedRows) {
      const group = groups.get(row.source.key) ?? { source: row.source, ids: [] };
      group.ids.push(row.id);
      groups.set(row.source.key, group);
    }
    const currentKey = currentSessionRef.current?.key;
    const wasCurrent = currentKey ? keySet.has(currentKey) : false;
    if (wasCurrent) clearInspection();
    resolveFailure("action");
    const failures: string[] = [];
    const orderedGroups = [...groups.values()].sort((left, right) =>
      left.source.key.localeCompare(right.source.key),
    );
    for (const { source, ids } of orderedGroups) {
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
  const dialogSources = [
    ...dialogSessions
      .reduce(
        (groups, session) => {
          const current = groups.get(session.source.key) ?? { source: session.source, count: 0 };
          current.count += 1;
          groups.set(session.source.key, current);
          return groups;
        },
        new Map<
          string,
          {
            source: SessionSource;
            count: number;
          }
        >(),
      )
      .values(),
  ].sort((left, right) => left.source.key.localeCompare(right.source.key));
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
    if (!listUnavailable && currentSessionRef.current) {
      void openSession(currentSessionRef.current, false);
    } else {
      void load("refresh");
    }
  }
  return (
    <div className={`${layout.page} ${layout.catalogPage} ${styles.sessionPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={tenantError ? retryTenants : error ? retryPageError : undefined}
      />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${currentSession ? layout.showsDetail : ""}`}>
        <aside className={`${layout.catalogPanel} ${styles.sessionCatalog}`} aria-label="Sessions">
          <div
            className={`${layout.toolbar} ${selectionMode ? styles.sessionSelectionToolbar : ""}`}
          >
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={layout.selectionCancel}
                  disabled={deletionBusy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={layout.toolbarSelectionActions}>
                  <span className={layout.selectionCount} title={`${selectedKeys.size} selected`}>
                    {selectedKeys.size} selected
                  </span>
                  <button
                    type="button"
                    className={layout.selectionAll}
                    onClick={toggleAllSessions}
                    disabled={sessions.length === 0 || deletionBusy}
                  >
                    {allSelected ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={layout.selectionDelete}
                    aria-label="Delete selected Sessions"
                    disabled={selectedKeys.size === 0 || mutationBusy}
                    onClick={() => setDialogKeys([...selectedKeys])}
                  >
                    <Trash2 size={14} aria-hidden="true" />
                    Delete selected
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className={layout.toolbarFilters}>
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={loadingTenants || deletionBusy}
                    label="Tenant"
                    onCommit={commitTenants}
                    options={tenantOptions}
                    pluralLabel="tenants"
                    selected={selectedTenants}
                    triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                    unavailableSummary={
                      loadingTenants
                        ? "Loading"
                        : sessionTenantMissing
                          ? "Not found"
                          : "Unavailable"
                    }
                  />
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={deletionBusy}
                    label="Coding Agent"
                    onCommit={commitAgents}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={selectedAgents}
                    triggerIcon={
                      selectedAgents.size === 1 ? (
                        <BrandIcon
                          brand={brandForAgent([...selectedAgents][0] ?? "codex")}
                          size={14}
                        />
                      ) : (
                        <Box size={14} aria-hidden="true" />
                      )
                    }
                  />
                </div>
                <div className={layout.toolbarActions}>
                  <RefreshButton
                    ref={refreshButton}
                    data-dialog-focus-fallback="true"
                    className={layout.refreshAction}
                    label="Refresh Sessions"
                    busyLabel="Refreshing Sessions"
                    busy={refreshing}
                    disabled={loadingList || refreshing || deletionBusy}
                    onClick={() => void load("refresh")}
                  >
                    Refresh
                  </RefreshButton>
                  <button
                    ref={selectButton}
                    type="button"
                    className={layout.selectionEnter}
                    aria-label="Select Sessions"
                    disabled={
                      sessions.length === 0 ||
                      unsafeView ||
                      loadingList ||
                      refreshing ||
                      deletionBusy
                    }
                    onClick={() => setSelectionMode(true)}
                  >
                    <ListChecks size={14} aria-hidden="true" />
                    Select
                  </button>
                </div>
              </>
            )}
          </div>
          <div className={styles.sessionWarnings}>
            {data?.warnings.map((warning) => (
              <div className={styles.inlineWarning} key={warning}>
                <AlertTriangle size={15} aria-hidden="true" />
                <span>{warning}</span>
              </div>
            ))}
          </div>
          <div className={`${styles.catalogList} ${styles.sessionList}`} aria-busy={loadingList}>
            {!data && loadingList && <Loading />}
            {sessions.map((row) => (
              <SessionRow
                key={row.key}
                row={row}
                current={currentSession?.key === row.key}
                selectionMode={selectionMode}
                selected={selectedKeys.has(row.key)}
                deleting={deletion?.kind === "record" && deletion.key === row.key}
                mutationBusy={mutationBusy}
                deletionBusy={deletionBusy}
                loadingList={loadingList}
                unsafeView={unsafeView}
                onOpen={() => void openSession(row)}
                onToggle={() => toggleSession(row.key)}
                onDelete={() => setSingleDeleteTarget(row)}
                registerRow={(element) => {
                  if (element) sessionRowButtons.current.set(row.key, element);
                  else sessionRowButtons.current.delete(row.key);
                }}
                registerDelete={(element) => {
                  if (element) deleteButtons.current.set(row.key, element);
                  else deleteButtons.current.delete(row.key);
                }}
              />
            ))}
            {data?.sessions.length === 0 && !loadingList && (
              <EmptyState
                variant="list"
                icon={<SessionIcon size={22} data-icon="session-list-empty" aria-hidden="true" />}
                title="No Sessions found"
                description="No Sessions were found for the selected Tenants and Coding Agents."
              />
            )}
          </div>
        </aside>
        <section className={styles.detailPane}>
          {currentSession ? (
            <>
              <header className={`${styles.detailHeader} ${styles.sessionDetailHeader}`}>
                <IconButton label="Back to Sessions" onClick={closeSessionInspection}>
                  <ChevronLeft size={17} />
                </IconButton>
                <div className={styles.sessionDetailHeading}>
                  <h2 ref={detailHeadingRef} tabIndex={-1}>
                    {currentSession.title || "Untitled Session"}
                  </h2>
                  <span className={styles.sessionDetailSource}>
                    {visibleSessionListSource(currentSession.source)} ·{" "}
                    <time dateTime={currentSession.start_ts}>
                      {formatTimestamp(currentSession.start_ts)}
                    </time>{" "}
                    · {compactDuration(detailStats?.observed_duration_ms)} ·{" "}
                    {messageCountLabel(
                      detailStats?.message_count ?? currentSession.message_count ?? 0,
                    )}{" "}
                    · {toolCountLabel(detailStats?.tool_count ?? currentSession.tool_count ?? 0)}
                  </span>
                </div>
                <div className={styles.sessionDetailActions}>
                  {loadingDetail && (
                    <span className={styles.sessionDetailStatus} role="status">
                      Reading Transcript…
                    </span>
                  )}
                  {!loadingDetail && !detailStats && (
                    <span
                      className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}
                    >
                      Partial transcript
                    </span>
                  )}
                  {!loadingDetail && detailStats && sessionWarnings.length > 0 && (
                    <span
                      className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}
                    >
                      <AlertTriangle size={13} aria-hidden="true" /> Transcript warning
                    </span>
                  )}
                  <RefreshButton
                    label="Refresh Session detail"
                    busyLabel="Refreshing Session detail"
                    busy={loadingDetail}
                    iconOnly
                    iconSize={15}
                    disabled={deletionBusy}
                    onClick={() => void openSession(currentSession, false, true)}
                  />
                </div>
              </header>
              <nav className={styles.sessionTabs} aria-label="Session views">
                <button
                  type="button"
                  className={sessionTab === "conversation" ? styles.sessionTabActive : undefined}
                  aria-current={sessionTab === "conversation" ? "page" : undefined}
                  onClick={() => updateSessionTab("conversation")}
                >
                  Conversation
                </button>
                <button
                  type="button"
                  className={sessionTab === "details" ? styles.sessionTabActive : undefined}
                  aria-current={sessionTab === "details" ? "page" : undefined}
                  onClick={() => updateSessionTab("details")}
                >
                  Details
                  {transcriptHasDiagnostics && (
                    <span
                      className={styles.sessionTabIssue}
                      aria-label="Transcript diagnostics"
                      title="Transcript diagnostics"
                    >
                      <AlertTriangle size={11} aria-hidden="true" />
                    </span>
                  )}
                </button>
              </nav>
              {sessionTab === "details" ? (
                <SessionDetails
                  session={currentSession}
                  meta={detailMeta}
                  stats={detailStats}
                  warnings={sessionWarnings}
                  loading={loadingDetail}
                  hasDiagnostics={transcriptHasDiagnostics}
                  partial={transcriptIsPartial}
                />
              ) : (
                <SessionConversation
                  api={api}
                  session={currentSession}
                  timeline={timeline}
                  userMessages={userMessages}
                  activeUserMessage={resolvedActiveUserMessage}
                  loading={loadingDetail}
                  warnings={sessionWarnings}
                  snapshot={detailStats?.snapshot}
                  revision={detailRevision}
                  showJumpLatest={showJumpLatest}
                  scrollRef={conversationScrollRef}
                  messageRefs={userMessageRefs}
                  onScroll={onConversationScroll}
                  onSelectMessage={jumpToUserMessage}
                  onJumpLatest={jumpToLatest}
                  onViewDiagnostics={() => updateSessionTab("details")}
                />
              )}
            </>
          ) : (
            <EmptyState
              variant="detail"
              icon={<SessionIcon size={26} data-icon="session-empty" aria-hidden="true" />}
              title="Select a Session"
              description="Choose a Session to inspect its conversation and Transcript."
            />
          )}
        </section>
      </div>
      <NotificationCenter
        notifications={notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogKeys !== null || singleDeleteTarget !== null}
        onAction={() => undefined}
        onDismiss={dismissNotification}
      />
      {singleDeleteTarget && (
        <ConfirmDialog
          title={`Delete Session ${singleDeleteTarget.display_id}?`}
          message={`This permanently deletes its Transcript from ${visibleSessionSource(singleDeleteTarget.source)}.`}
          confirmLabel="Delete permanently"
          busy={deletion?.kind === "record" || operation?.state === "running"}
          onCancel={() => {
            if (deletion?.kind !== "record") setSingleDeleteTarget(null);
          }}
          onConfirm={() => void deleteSession(singleDeleteTarget)}
        />
      )}
      {dialogKeys && (
        <ConfirmDialog
          title={`Delete ${dialogKeys.length} selected Session${dialogKeys.length === 1 ? "" : "s"}?`}
          message={`This permanently deletes the Transcripts for the selected Sessions. Sources: ${dialogSources
            .map(({ count, source }) => `${visibleSessionSource(source)} (${count})`)
            .join("; ")}.`}
          confirmLabel="Delete permanently"
          busy={batchBusy || operation?.state === "running"}
          onCancel={() => {
            if (!batchBusy) setDialogKeys(null);
          }}
          onConfirm={() => void deleteSelectedSessions()}
        />
      )}
    </div>
  );
}
