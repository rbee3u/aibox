import {
  AlertTriangle,
  ArrowDown,
  Box,
  Check,
  Clipboard,
  ChevronLeft,
  ListChecks,
  LoaderCircle,
  RefreshCw,
  Trash2,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { ReactNode, UIEvent } from "react";
import type {
  CodingAgentKind,
  ConversationMessage,
  Operation,
  TenantSelection,
  SessionApi,
  SessionDetailMeta,
  SessionDetailStats,
  SessionRow,
  ToolActivity,
  TranscriptEvidence,
  TranscriptEvidenceSummary,
  TenantRow,
} from "./controlApi";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { EmptyState } from "./components/EmptyState";
import { IconButton } from "./components/IconButton";
import { NotificationCenter } from "./components/NotificationCenter";
import { Loading, MutationUnavailable, PageError } from "./components/ManagementFeedback";
import { SelectionMenu, type SelectionOption } from "./components/SelectionMenu";
import { SessionMessageContent } from "./components/SessionMessageContent";
import { useClipboardFeedback } from "./useClipboardFeedback";
import { useFailureNotifications } from "./useFailureNotifications";
import { AgentIcon } from "./icons";
import { bytes, compactDuration, formatTimestamp } from "./utils";
import { resourceIcons, type ModuleId } from "./consoleIcons";
import {
  changePageLocation,
  messageOf,
  parseTenantSelectionKey,
  useTenants,
} from "./managementSupport";
import styles from "./SessionPage.module.css";
const SessionIcon = resourceIcons.session;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
interface PageProps {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange?: (module: ModuleId, query: URLSearchParams, replace?: boolean) => void;
}
type SessionTenantKey = "host" | `managed:${string}`;
interface SessionSource {
  key: string;
  tenant: TenantSelection;
  tenantKey: SessionTenantKey;
  tenantLabel: string;
  agent: CodingAgentKind;
  agentLabel: string;
}
interface SourcedSession extends SessionRow {
  key: string;
  source: SessionSource;
}
interface AggregatedSessionData {
  sessions: SourcedSession[];
  warnings: string[];
  partial: boolean;
}
export type SessionTimelineItem =
  | {
      kind: "message";
      value: ConversationMessage;
    }
  | {
      kind: "activity";
      value: SessionActivityItem[];
    };
export type SessionActivityItem =
  | {
      kind: "tool";
      value: ToolActivity;
    }
  | {
      kind: "evidence";
      value: TranscriptEvidenceSummary;
    };
function sessionItemKey(item: SessionTimelineItem): string {
  if (item.kind === "message") return `message:${item.value.entry_ids.join(",")}`;
  return `activity:${item.value
    .map((entry) =>
      entry.kind === "tool"
        ? `tool:${entry.value.entry_ids.join(",")}:${entry.value.status}`
        : `evidence:${entry.value.entry_id}`,
    )
    .join(",")}`;
}
function conversationIsAwayFromLatest(element: HTMLDivElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight > 160;
}
function appendActivityItem(
  current: SessionTimelineItem[],
  entry: SessionActivityItem,
): SessionTimelineItem[] {
  const last = current.at(-1);
  if (entry.kind === "tool" && entry.value.status !== "started" && entry.value.call_id) {
    for (let cursor = current.length - 1; cursor >= 0; cursor -= 1) {
      const item = current[cursor];
      if (item.kind !== "activity") continue;
      const entryIndex = item.value.findIndex(
        (candidate) => candidate.kind === "tool" && candidate.value.call_id === entry.value.call_id,
      );
      if (entryIndex < 0) continue;
      const nextActivity = [...item.value];
      const existing = nextActivity[entryIndex];
      if (existing.kind === "tool") {
        nextActivity[entryIndex] = {
          kind: "tool",
          value: {
            ...existing.value,
            entry_ids: [...existing.value.entry_ids, ...entry.value.entry_ids],
            status: entry.value.status,
            summary: entry.value.summary || existing.value.summary,
          },
        };
      }
      const next = [...current];
      next[cursor] = { kind: "activity", value: nextActivity };
      return next;
    }
  }
  if (last?.kind === "activity") {
    return [...current.slice(0, -1), { kind: "activity", value: [...last.value, entry] }];
  }
  return [...current, { kind: "activity", value: [entry] }];
}
function appendConversationMessage(
  current: SessionTimelineItem[],
  message: ConversationMessage,
): SessionTimelineItem[] {
  const last = current.at(-1);
  if (message.role === "assistant" && last?.kind === "message" && last.value.role === "assistant") {
    return [
      ...current.slice(0, -1),
      {
        kind: "message",
        value: {
          ...last.value,
          entry_ids: [...last.value.entry_ids, ...message.entry_ids],
          timestamp: message.timestamp || last.value.timestamp,
          text: `${last.value.text}\n\n${message.text}`,
        },
      },
    ];
  }
  return [...current, { kind: "message", value: message }];
}
function messageNavigationLabel(text: string): string {
  const firstLine = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  const value = firstLine || text.trim() || "Untitled message";
  return value.length > 96 ? `${value.slice(0, 93)}…` : value;
}
function messageAnchorId(message: ConversationMessage): string {
  return `session-message-${message.entry_ids[0]?.replace(/[^a-zA-Z0-9_-]/g, "-") || "unknown"}`;
}
function activitySummary(entries: SessionActivityItem[]): {
  count: number;
  labels: string[];
  hasIssue: boolean;
} {
  const labels = [
    ...new Set(
      entries
        .map((entry) => (entry.kind === "tool" ? entry.value.name : entry.value.native_type))
        .filter(Boolean),
    ),
  ];
  const hasIssue = entries.some((entry) => {
    if (entry.kind === "tool") {
      return !["started", "completed"].includes(entry.value.status);
    }
    return ["malformed", "unsupported", "hidden_internal"].includes(entry.value.status);
  });
  return { count: entries.length, labels, hasIssue };
}
function SessionEvidenceDisclosure({
  api,
  entryId,
  label,
  meta,
  preview,
  session,
  snapshot,
  status,
}: {
  api: SessionApi;
  entryId: string;
  label: ReactNode;
  meta: string;
  preview: string;
  session: SourcedSession;
  snapshot?: string;
  status: string;
}) {
  const [evidence, setEvidence] = useState<TranscriptEvidence | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hidden = status === "hidden_internal";
  async function loadEvidence() {
    if (evidence || loading || hidden || !snapshot) return;
    setLoading(true);
    setError(null);
    try {
      setEvidence(
        await api.loadSessionEvidence(
          session.source.tenant,
          session.source.agent,
          session.id,
          entryId,
          snapshot,
        ),
      );
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setLoading(false);
    }
  }
  return (
    <details
      className={status === "tool" ? styles.sessionActivity : styles.sessionEvidence}
      onToggle={(event) => {
        if (event.currentTarget.open) void loadEvidence();
      }}
    >
      <summary>
        <span>{label}</span>
        <span>{meta}</span>
      </summary>
      {preview && <pre>{preview}</pre>}
      {hidden && <p>Internal reasoning is intentionally hidden.</p>}
      {!hidden && !snapshot && (
        <p>Full evidence is available after the Transcript finishes loading.</p>
      )}
      {loading && <p>Loading Transcript Entry…</p>}
      {error && <p className={styles.sessionEvidenceError}>{error}</p>}
      {evidence && (
        <div className={styles.sessionEvidenceRaw}>
          <button
            type="button"
            onClick={() => void navigator.clipboard.writeText(evidence.content)}
          >
            <Clipboard size={13} aria-hidden="true" /> Copy {evidence.encoding}
          </button>
          <pre>{evidence.content}</pre>
        </div>
      )}
    </details>
  );
}
function SessionActivityGroup({
  api,
  entries,
  reloadRevision,
  session,
  snapshot,
}: {
  api: SessionApi;
  entries: SessionActivityItem[];
  reloadRevision: number;
  session: SourcedSession;
  snapshot?: string;
}) {
  const disclosureRef = useRef<HTMLDetailsElement>(null);
  const summary = activitySummary(entries);
  const activityLabels =
    summary.labels.length > 0
      ? `${summary.labels.slice(0, 3).join(", ")}${summary.labels.length > 3 ? ` +${summary.labels.length - 3}` : ""}`
      : "Transcript events";
  useEffect(() => {
    if (disclosureRef.current) disclosureRef.current.open = false;
  }, [reloadRevision]);
  return (
    <details ref={disclosureRef} className={styles.sessionActivityGroup}>
      <summary>
        <span>
          <Wrench size={13} aria-hidden="true" /> Transcript activity
          {summary.hasIssue && <AlertTriangle size={13} aria-label="Activity has diagnostics" />}
        </span>
        <span>
          {summary.count} {summary.count === 1 ? "item" : "items"} · {activityLabels}
        </span>
      </summary>
      <div className={styles.sessionActivityGroupItems}>
        {entries.map((entry) =>
          entry.kind === "tool" ? (
            <SessionEvidenceDisclosure
              key={`tool:${entry.value.entry_ids.join(",")}`}
              api={api}
              entryId={entry.value.entry_ids[0]}
              label={
                <>
                  <Wrench size={13} aria-hidden="true" /> {entry.value.name}
                </>
              }
              meta={
                ["started", "completed"].includes(entry.value.status)
                  ? compactMessageTimestamp(entry.value.timestamp, session.start_ts)
                  : entry.value.status
              }
              preview={entry.value.summary}
              session={session}
              snapshot={snapshot}
              status="tool"
            />
          ) : (
            <SessionEvidenceDisclosure
              key={entry.value.entry_id}
              api={api}
              entryId={entry.value.entry_id}
              label={entry.value.native_type}
              meta={`${entry.value.status} · ${compactMessageTimestamp(entry.value.timestamp, session.start_ts)}`}
              preview={entry.value.preview}
              session={session}
              snapshot={snapshot}
              status={entry.value.status}
            />
          ),
        )}
      </div>
    </details>
  );
}
function SessionConversationNav({
  messages,
  activeEntryId,
  mobile = false,
  onSelect,
}: {
  messages: ConversationMessage[];
  activeEntryId: string | null;
  mobile?: boolean;
  onSelect: (entryId: string) => void;
}) {
  if (messages.length === 0) return null;
  return (
    <nav
      className={mobile ? styles.sessionConversationMobileNav : styles.sessionConversationRail}
      aria-label="Conversation messages"
    >
      <div className={styles.sessionConversationNavItems}>
        {messages.map((message, index) => {
          const entryId = message.entry_ids[0] ?? `message-${index}`;
          const label = messageNavigationLabel(message.text);
          return (
            <button
              key={entryId}
              type="button"
              className={
                activeEntryId === entryId ? styles.sessionConversationNavActive : undefined
              }
              aria-current={activeEntryId === entryId ? "location" : undefined}
              aria-label={`Jump to message ${index + 1}: ${label}`}
              title={label}
              onClick={() => onSelect(entryId)}
            >
              <span className={styles.sessionConversationNavDot} aria-hidden="true">
                <span />
              </span>
              <span className={styles.sessionConversationNavIndex}>{index + 1}</span>
              <span className={styles.sessionConversationNavLabel}>{label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
function SessionCopyValue({ label, value }: { label: string; value: string }) {
  const [copied, copy] = useClipboardFeedback();
  return (
    <span className={styles.sessionCopyValue}>
      <code>{value}</code>
      <IconButton
        className={styles.sessionCopyAction}
        label={copied ? `${label} copied` : `Copy ${label}`}
        onClick={() => void copy(value, true)}
      >
        {copied ? (
          <Check size={13} aria-hidden="true" />
        ) : (
          <Clipboard size={13} aria-hidden="true" />
        )}
      </IconButton>
    </span>
  );
}
function compactMessageTimestamp(value: string, sessionStart: string): string {
  const formatted = formatTimestamp(value);
  if (formatted === "—") return formatted;
  const start = formatTimestamp(sessionStart);
  const [date, time] = formatted.split(" ");
  const [startDate] = start.split(" ");
  return date === startDate ? time : formatted;
}
function messageCountLabel(count: number): string {
  return `${count} message${count === 1 ? "" : "s"}`;
}
function toolCountLabel(count: number): string {
  return `${count} tool${count === 1 ? "" : "s"}`;
}
type SessionTab = "conversation" | "details";
type SessionDeletion =
  | {
      kind: "record";
      key: string;
    }
  | {
      kind: "batch";
    }
  | null;
const SESSION_AGENT_OPTIONS: readonly {
  value: CodingAgentKind;
  label: string;
}[] = [
  { value: "codex", label: "Codex" },
  { value: "claude", label: "Claude" },
];
function agentLabel(agent: CodingAgentKind): string {
  return SESSION_AGENT_OPTIONS.find((option) => option.value === agent)?.label ?? agent;
}
function tenantSelectionFromSessionKey(key: SessionTenantKey): TenantSelection {
  return key === "host" ? { kind: "host" } : { kind: "managed", name: key.slice(8) };
}
function sessionTenantLabel(key: SessionTenantKey): string {
  return key === "host" ? "Host Tenant" : `Tenant ${key.slice(8)}`;
}
function sessionListTenantLabel(key: SessionTenantKey): string {
  return key === "host" ? "Host Tenant" : key.slice(8);
}
function visibleSessionSource(source: SessionSource): string {
  return `${source.tenantLabel} ${source.agentLabel}`;
}
function visibleSessionListSource(source: SessionSource): string {
  return `${sessionListTenantLabel(source.tenantKey)} ${source.agentLabel}`;
}
function accessibleSessionSource(source: SessionSource): string {
  return `${source.tenantLabel} · ${source.agentLabel}`;
}
function sessionSource(tenantKey: SessionTenantKey, agent: CodingAgentKind): SessionSource {
  return {
    key: JSON.stringify([tenantKey, agent]),
    tenant: tenantSelectionFromSessionKey(tenantKey),
    tenantKey,
    tenantLabel: sessionTenantLabel(tenantKey),
    agent,
    agentLabel: agentLabel(agent),
  };
}
interface SessionRouteSelection {
  tenantKey: SessionTenantKey;
  agent: CodingAgentKind;
  id: string;
}
interface SessionRouteState {
  tenants: Set<SessionTenantKey>;
  agents: Set<CodingAgentKind>;
  selection: SessionRouteSelection | null;
  tab: SessionTab;
}
function readSessionRoute(search: string): SessionRouteState {
  const query = new URLSearchParams(search);
  const tenants = new Set(
    query
      .getAll("tenant")
      .map(parseTenantSelectionKey)
      .filter((value): value is SessionTenantKey => value !== null),
  );
  const agents = new Set(
    query
      .getAll("agent")
      .filter((value): value is CodingAgentKind => value === "codex" || value === "claude"),
  );
  if (tenants.size === 0) tenants.add("managed:default");
  if (agents.size === 0) agents.add("codex");
  const selectedTenant = parseTenantSelectionKey(query.get("session_tenant"));
  const selectedAgent = query.get("session_agent");
  const id = query.get("session");
  const selection: SessionRouteSelection | null =
    selectedTenant && (selectedAgent === "codex" || selectedAgent === "claude") && id
      ? { tenantKey: selectedTenant, agent: selectedAgent, id }
      : null;
  const tab = query.get("tab") === "details" ? "details" : "conversation";
  return { tenants, agents, selection, tab };
}
function sessionLocation(
  tenants: ReadonlySet<SessionTenantKey>,
  agents: ReadonlySet<CodingAgentKind>,
  selection?: SessionRouteSelection | null,
  tab: SessionTab = "conversation",
): URLSearchParams {
  const query = new URLSearchParams();
  for (const tenant of [...tenants].sort()) query.append("tenant", tenant);
  for (const agent of SESSION_AGENT_OPTIONS.map((option) => option.value)) {
    if (agents.has(agent)) query.append("agent", agent);
  }
  if (selection) {
    query.set("session_tenant", selection.tenantKey);
    query.set("session_agent", selection.agent);
    query.set("session", selection.id);
    if (tab === "details") query.set("tab", tab);
  }
  return query;
}
function sourcedSession(source: SessionSource, row: SessionRow): SourcedSession {
  return {
    ...row,
    key: JSON.stringify([source.tenantKey, source.agent, row.id]),
    source,
  };
}
function compareSessions(left: SourcedSession, right: SourcedSession): number {
  return (
    right.start_ts.localeCompare(left.start_ts) ||
    left.source.tenantLabel.localeCompare(right.source.tenantLabel) ||
    left.source.agentLabel.localeCompare(right.source.agentLabel) ||
    left.id.localeCompare(right.id)
  );
}
function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}
function focusTargetAfterSessionDelete(rows: SourcedSession[], key: string): string | null {
  const index = rows.findIndex((row) => row.key === key);
  if (index < 0) return null;
  return rows[index + 1]?.key ?? rows[index - 1]?.key ?? null;
}

export interface SessionDetailState {
  timeline: SessionTimelineItem[];
  meta: SessionDetailMeta | null;
  stats: SessionDetailStats | null;
  warnings: string[];
  loading: boolean;
}

export type SessionDetailAction =
  | { type: "reset" }
  | { type: "start"; preserveContent: boolean }
  | { type: "stop" }
  | { type: "meta"; value: SessionDetailMeta }
  | { type: "message"; value: ConversationMessage }
  | { type: "activity"; value: SessionActivityItem }
  | { type: "complete"; stats: SessionDetailStats; warnings: string[] }
  | {
      type: "replace";
      timeline: SessionTimelineItem[];
      meta: SessionDetailMeta | null;
      stats: SessionDetailStats | null;
      warnings: string[];
    };

export const emptySessionDetail: SessionDetailState = {
  timeline: [],
  meta: null,
  stats: null,
  warnings: [],
  loading: false,
};

export function sessionDetailReducer(
  state: SessionDetailState,
  action: SessionDetailAction,
): SessionDetailState {
  switch (action.type) {
    case "reset":
      return emptySessionDetail;
    case "start":
      return action.preserveContent
        ? { ...state, loading: true }
        : { ...emptySessionDetail, loading: true };
    case "stop":
      return state.loading ? { ...state, loading: false } : state;
    case "meta":
      return { ...state, meta: action.value };
    case "message":
      return { ...state, timeline: appendConversationMessage(state.timeline, action.value) };
    case "activity":
      return { ...state, timeline: appendActivityItem(state.timeline, action.value) };
    case "complete":
      return { ...state, stats: action.stats, warnings: action.warnings };
    case "replace":
      return {
        timeline: action.timeline,
        meta: action.meta,
        stats: action.stats,
        warnings: action.warnings,
        loading: state.loading,
      };
  }
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
      changePageLocation("sessions", query, onLocationChange, replace);
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
        icon: <AgentIcon agent={option.value} size={14} />,
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
    <div className={`${styles.page} ${styles.catalogPage} ${styles.sessionPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={tenantError ? retryTenants : error ? retryPageError : undefined}
      />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${currentSession ? styles.hasSelection : ""}`}>
        <aside className={`${styles.catalog} ${styles.sessionCatalog}`} aria-label="Sessions">
          <div
            className={`${styles.sessionToolbar} ${selectionMode ? styles.sessionSelectionToolbar : ""}`}
          >
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={styles.sessionCancelSelection}
                  disabled={deletionBusy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={styles.sessionSelectionActions}>
                  <span
                    className={styles.sessionSelectionCount}
                    title={`${selectedKeys.size} selected`}
                  >
                    {selectedKeys.size} selected
                  </span>
                  <button
                    type="button"
                    className={styles.sessionSelectAll}
                    onClick={toggleAllSessions}
                    disabled={sessions.length === 0 || deletionBusy}
                  >
                    {allSelected ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={styles.sessionDeleteSelected}
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
                <div className={styles.sessionFilters}>
                  <SelectionMenu
                    className={styles.sessionTenantFilter}
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
                    className={styles.sessionAgentFilter}
                    disabled={deletionBusy}
                    label="Coding Agent"
                    onCommit={commitAgents}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={selectedAgents}
                    triggerIcon={
                      selectedAgents.size === 1 ? (
                        <AgentIcon agent={[...selectedAgents][0] ?? "codex"} size={14} />
                      ) : (
                        <Box size={14} aria-hidden="true" />
                      )
                    }
                  />
                </div>
                <div className={styles.sessionHeaderActions}>
                  <IconButton
                    buttonRef={refreshButton}
                    data-dialog-focus-fallback="true"
                    className={styles.sessionRefresh}
                    label={refreshing ? "Refreshing Sessions" : "Refresh Sessions"}
                    aria-busy={refreshing}
                    disabled={loadingList || refreshing || deletionBusy}
                    onClick={() => void load("refresh")}
                  >
                    <RefreshCw
                      className={refreshing ? "spin" : undefined}
                      size={14}
                      aria-hidden="true"
                    />
                  </IconButton>
                  <button
                    ref={selectButton}
                    type="button"
                    className={styles.sessionSelect}
                    aria-label="Select Sessions"
                    title="Select Sessions"
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
            {sessions.map((row) => {
              const selectedForDeletion = selectedKeys.has(row.key);
              const deleting = deletion?.kind === "record" && deletion.key === row.key;
              const title = row.title || "Untitled Session";
              const visibleSourceDescription = visibleSessionSource(row.source);
              const accessibleSourceDescription = accessibleSessionSource(row.source);
              return (
                <div
                  key={row.key}
                  className={[
                    styles.sessionRow,
                    currentSession?.key === row.key ? styles.currentSessionRow : "",
                    selectionMode ? styles.sessionSelectionRow : "",
                    selectedForDeletion ? styles.sessionRowSelected : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                >
                  <button
                    ref={(element) => {
                      if (element) sessionRowButtons.current.set(row.key, element);
                      else sessionRowButtons.current.delete(row.key);
                    }}
                    type="button"
                    className={styles.sessionRowMain}
                    aria-label={
                      selectionMode
                        ? `${selectedForDeletion ? "Deselect" : "Select"} ${title}, ${accessibleSourceDescription}`
                        : `${title}, ${accessibleSourceDescription}`
                    }
                    aria-pressed={selectionMode ? selectedForDeletion : undefined}
                    disabled={deletionBusy || loadingList}
                    onClick={() => (selectionMode ? toggleSession(row.key) : void openSession(row))}
                  >
                    <SessionIcon size={16} data-icon="session-record" aria-hidden="true" />
                    <span>
                      <strong title={title}>{title}</strong>
                      <small className={styles.sessionRowMetadata}>
                        <span>{visibleSessionListSource(row.source)}</span>
                        <time dateTime={row.start_ts}>{formatTimestamp(row.start_ts)}</time>
                      </small>
                      <small className={styles.sessionRowPreview} title={row.latest_message ?? ""}>
                        {row.latest_message || "No readable conversation content"}
                      </small>
                    </span>
                    {row.warnings.length > 0 && (
                      <span
                        className={styles.sessionRowWarning}
                        role="img"
                        aria-label={`Session has ${row.warnings.length} Transcript warning${row.warnings.length === 1 ? "" : "s"}`}
                        title={row.warnings.join("\n")}
                      >
                        <AlertTriangle size={14} aria-hidden="true" />
                      </span>
                    )}
                    {selectionMode && (
                      <span className={styles.sessionSelectionIndicator} aria-hidden="true">
                        {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                      </span>
                    )}
                  </button>
                  {!selectionMode && (
                    <button
                      ref={(element) => {
                        if (element) deleteButtons.current.set(row.key, element);
                        else deleteButtons.current.delete(row.key);
                      }}
                      type="button"
                      className={styles.sessionDelete}
                      title={`Delete Session ${row.display_id} from ${visibleSourceDescription}`}
                      aria-label={
                        deleting
                          ? `Deleting Session ${row.display_id} from ${accessibleSourceDescription}`
                          : `Delete Session ${row.display_id} from ${accessibleSourceDescription}`
                      }
                      aria-busy={deleting}
                      disabled={unsafeView || mutationBusy || loadingList}
                      onClick={() => setSingleDeleteTarget(row)}
                    >
                      {deleting ? (
                        <LoaderCircle className="spin" size={15} aria-hidden="true" />
                      ) : (
                        <Trash2 size={15} aria-hidden="true" />
                      )}
                    </button>
                  )}
                </div>
              );
            })}
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
                  <IconButton
                    label="Refresh Session detail"
                    disabled={deletionBusy}
                    onClick={() => void openSession(currentSession, false, true)}
                  >
                    <RefreshCw className={loadingDetail ? "spin" : undefined} size={15} />
                  </IconButton>
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
                <div className={styles.sessionDetailsScroll}>
                  <div className={styles.sessionDetailsContent}>
                    <section className={styles.sessionDetailsSection}>
                      <h3>Session</h3>
                      <dl className={styles.sessionDetailsGrid}>
                        <div>
                          <dt>Tenant</dt>
                          <dd>{sessionListTenantLabel(currentSession.source.tenantKey)}</dd>
                        </div>
                        <div>
                          <dt>Coding Agent</dt>
                          <dd>{currentSession.source.agentLabel}</dd>
                        </div>
                        <div>
                          <dt>Session ID</dt>
                          <dd>
                            <SessionCopyValue
                              label="Session ID"
                              value={detailMeta?.id ?? currentSession.id}
                            />
                          </dd>
                        </div>
                        {detailMeta?.transcript_path && (
                          <div>
                            <dt>Transcript</dt>
                            <dd>
                              <SessionCopyValue
                                label="Transcript path"
                                value={detailMeta.transcript_path}
                              />
                            </dd>
                          </div>
                        )}
                        {detailMeta?.cwd && (
                          <div>
                            <dt>Working directory</dt>
                            <dd>
                              <SessionCopyValue label="Working directory" value={detailMeta.cwd} />
                            </dd>
                          </div>
                        )}
                        <div>
                          <dt>Started</dt>
                          <dd>
                            <time dateTime={detailStats?.start_ts ?? currentSession.start_ts}>
                              {formatTimestamp(detailStats?.start_ts ?? currentSession.start_ts)}
                            </time>
                          </dd>
                        </div>
                        {detailStats?.last_event_ts && (
                          <div>
                            <dt>Last event</dt>
                            <dd>
                              <time dateTime={detailStats.last_event_ts}>
                                {formatTimestamp(detailStats.last_event_ts)}
                              </time>
                            </dd>
                          </div>
                        )}
                        {detailStats && (
                          <div>
                            <dt>Duration</dt>
                            <dd>{compactDuration(detailStats.observed_duration_ms)}</dd>
                          </div>
                        )}
                        {detailStats && (
                          <div>
                            <dt>Transcript size</dt>
                            <dd>{bytes(detailStats.file_size)}</dd>
                          </div>
                        )}
                        {detailMeta?.model_provider && (
                          <div>
                            <dt>Model provider</dt>
                            <dd>{detailMeta.model_provider}</dd>
                          </div>
                        )}
                        {detailMeta?.cli_version && (
                          <div>
                            <dt>CLI version</dt>
                            <dd>{detailMeta.cli_version}</dd>
                          </div>
                        )}
                      </dl>
                    </section>
                    <section className={styles.sessionDetailsSection}>
                      <div className={styles.sessionDetailsSectionHeading}>
                        <h3>Diagnostics</h3>
                        {loadingDetail ? (
                          <span>Reading Transcript…</span>
                        ) : (
                          !transcriptHasDiagnostics && <span>No transcript diagnostics.</span>
                        )}
                      </div>
                      {detailStats && transcriptHasDiagnostics && (
                        <dl className={styles.sessionDiagnosticsGrid}>
                          <div>
                            <dt>Transcript entries</dt>
                            <dd>{detailStats.entry_count}</dd>
                          </div>
                          {detailStats.malformed_count > 0 && (
                            <div>
                              <dt>Malformed</dt>
                              <dd>{detailStats.malformed_count}</dd>
                            </div>
                          )}
                          {detailStats.unsupported_count > 0 && (
                            <div>
                              <dt>Unsupported</dt>
                              <dd>{detailStats.unsupported_count}</dd>
                            </div>
                          )}
                          {detailStats.hidden_internal_count > 0 && (
                            <div>
                              <dt>Hidden internal</dt>
                              <dd>{detailStats.hidden_internal_count}</dd>
                            </div>
                          )}
                        </dl>
                      )}
                      {sessionWarnings.length > 0 && (
                        <div className={styles.sessionDiagnosticWarnings}>
                          {sessionWarnings.map((warning) => (
                            <p key={warning}>{warning}</p>
                          ))}
                        </div>
                      )}
                      {transcriptIsPartial && (
                        <div className={styles.sessionDiagnosticWarnings}>
                          <p>
                            Transcript detail did not finish loading. Displayed content may be
                            incomplete.
                          </p>
                        </div>
                      )}
                    </section>
                  </div>
                </div>
              ) : (
                <div className={styles.sessionConversationLayout}>
                  <SessionConversationNav
                    messages={userMessages}
                    activeEntryId={resolvedActiveUserMessage}
                    onSelect={jumpToUserMessage}
                  />
                  <div className={styles.sessionConversationMain}>
                    <SessionConversationNav
                      messages={userMessages}
                      activeEntryId={resolvedActiveUserMessage}
                      mobile
                      onSelect={jumpToUserMessage}
                    />
                    <div
                      ref={conversationScrollRef}
                      className={styles.sessionConversationScroll}
                      onScroll={onConversationScroll}
                    >
                      <div key={detailRevision} className={styles.sessionConversationContent}>
                        {sessionWarnings.length > 0 && (
                          <button
                            type="button"
                            className={styles.sessionConversationWarning}
                            onClick={() => updateSessionTab("details")}
                          >
                            <AlertTriangle size={14} aria-hidden="true" />
                            <span>Some transcript events could not be interpreted.</span>
                            <span>View Details</span>
                          </button>
                        )}
                        {timeline.map((item) => {
                          if (item.kind === "message") {
                            const label =
                              item.value.role === "user" ? "You" : currentSession.source.agentLabel;
                            const timestamp = compactMessageTimestamp(
                              item.value.timestamp,
                              currentSession.start_ts,
                            );
                            return (
                              <article
                                key={sessionItemKey(item)}
                                id={
                                  item.value.role === "user"
                                    ? messageAnchorId(item.value)
                                    : undefined
                                }
                                ref={(element) => {
                                  if (item.value.role !== "user") return;
                                  const entryId = item.value.entry_ids[0];
                                  if (!entryId) return;
                                  if (element) userMessageRefs.current.set(entryId, element);
                                  else userMessageRefs.current.delete(entryId);
                                }}
                                className={`${styles.sessionMessage} ${item.value.role === "user" ? styles.sessionMessageUser : styles.sessionMessageAssistant}`}
                              >
                                <header>
                                  <span>{label}</span>
                                  <time
                                    dateTime={item.value.timestamp}
                                    title={formatTimestamp(item.value.timestamp)}
                                  >
                                    {timestamp}
                                  </time>
                                </header>
                                <SessionMessageContent
                                  role={item.value.role}
                                  text={item.value.text}
                                />
                              </article>
                            );
                          }
                          return (
                            <SessionActivityGroup
                              key={sessionItemKey(item)}
                              api={api}
                              entries={item.value}
                              reloadRevision={detailRevision}
                              session={currentSession}
                              snapshot={detailStats?.snapshot}
                            />
                          );
                        })}
                        {loadingDetail && <Loading />}
                        {!loadingDetail && timeline.length === 0 && (
                          <EmptyState
                            className={styles.promptEmptyState}
                            variant="detail"
                            icon={<SessionIcon size={26} aria-hidden="true" />}
                            title="No readable conversation"
                            description="This Transcript contains no supported user or Coding Agent messages. Transcript events remain available below when present."
                          />
                        )}
                      </div>
                      {showJumpLatest && (
                        <IconButton
                          className={styles.jumpLatest}
                          label="Jump to latest"
                          onClick={jumpToLatest}
                        >
                          <ArrowDown size={16} aria-hidden="true" />
                        </IconButton>
                      )}
                    </div>
                  </div>
                </div>
              )}
            </>
          ) : (
            <EmptyState
              variant="detail"
              icon={<SessionIcon size={26} data-icon="session-empty" aria-hidden="true" />}
              title="Select a Session"
              description="Choose a Session to inspect its conversation and Transcript evidence."
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
