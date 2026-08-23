import {
  formatBytes,
  tenantSelectionValue,
  type CodingAgentKind,
  type ComponentRow,
  type Operation,
  type OverviewData,
  type SessionSummaryData,
  type TenantSelection,
  type TopologyAgent,
  type TopologyData,
  type TopologyTenant,
} from "./controlApi";
import type { ModuleId } from "./consoleIcons";
import { formatTimestamp } from "./utils";

export const MIN_ZOOM = 0.65;
export const MAX_ZOOM = 1.5;

export type Tone = "good" | "neutral" | "warning" | "error";
export type TreeIcon =
  | "service"
  | "host"
  | "tenant"
  | "codex"
  | "claude"
  | "current"
  | "configs"
  | "config"
  | "sessions"
  | "session-summary"
  | "components"
  | "component";
export interface NavigationTarget {
  module: ModuleId;
  query?: URLSearchParams;
}
export interface AttentionItem {
  label: string;
  detail: string;
  tone: "warning" | "error";
  target?: NavigationTarget;
}
export interface SessionRequest {
  tenant: TenantSelection;
  agent: CodingAgentKind;
}
export interface TopologyNode {
  id: string;
  parentId: string | null;
  label: string;
  detail?: string;
  title?: string;
  icon: TreeIcon;
  tone: Tone;
  target?: NavigationTarget;
  sessionRequest?: SessionRequest;
  children: TopologyNode[];
}
export type TopologyNodeKind = "entity" | "group" | "leaf";
export interface VisibleTopologyNode {
  node: TopologyNode;
  children: VisibleTopologyNode[];
  depth: number;
  open: boolean;
  branch: boolean;
  position: number;
  setSize: number;
}
export interface TopologyLayoutNode extends VisibleTopologyNode {
  x: number;
  y: number;
  width: number;
  height: number;
  kind: TopologyNodeKind;
}
export interface TopologyLayoutEdge {
  id: string;
  parentId: string;
  childId: string;
  path: string;
  tone: Tone;
}
export interface TopologyLayout {
  width: number;
  height: number;
  nodes: TopologyLayoutNode[];
  edges: TopologyLayoutEdge[];
}
export interface TopologySearchResult {
  matches: Set<string>;
  context: Set<string>;
  firstMatch: string | null;
}
export interface SessionLoad {
  state: "loading" | "loaded" | "error";
  data?: SessionSummaryData;
  error?: string;
}
export interface TopologyHealth {
  configTotal: number;
  configAttention: number;
  configErrors: number;
  componentInstalled: number;
  componentAttention: number;
  componentErrors: number;
}
export interface TopologyMetrics {
  layoutWidth: number;
  viewportWidth: number;
}
export function sessionAnnouncement(loads: Record<string, SessionLoad>): string {
  const loading = Object.values(loads).filter((load) => load.state === "loading").length;
  if (loading) return `Discovering ${loading} Session ${loading === 1 ? "summary" : "summaries"}`;
  const latest = Object.values(loads).at(-1);
  if (!latest) return "";
  if (latest.state === "error") return "Session summary unavailable";
  return latest.data ? `${latest.data.count} Sessions discovered` : "";
}
export function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(value * 10) / 10));
}
export function fitTopologyZoom(canvasWidth: number, viewportWidth: number): number {
  return clampZoom((viewportWidth - 32) / canvasWidth);
}
export function visibleTopology(
  node: TopologyNode,
  expanded: ReadonlySet<string>,
  forcedExpanded: ReadonlySet<string>,
  depth = 0,
  position = 1,
  setSize = 1,
): VisibleTopologyNode {
  const branch = node.children.length > 0 || Boolean(node.sessionRequest);
  const open = node.id === "service" || expanded.has(node.id) || forcedExpanded.has(node.id);
  const children = open
    ? node.children.map((child, index) =>
        visibleTopology(
          child,
          expanded,
          forcedExpanded,
          depth + 1,
          index + 1,
          node.children.length,
        ),
      )
    : [];
  return { node, children, depth, open, branch, position, setSize };
}
export function layoutTopology(root: VisibleTopologyNode, availableWidth: number): TopologyLayout {
  const PADDING_X = 32;
  const PADDING_Y = 28;
  const MIN_LEVEL_GAP = 72;
  const MAX_LEVEL_GAP = 220;
  const SUBTREE_GAP = 20;
  const levels: VisibleTopologyNode[][] = [];
  const visitLevels = (entry: VisibleTopologyNode) => {
    (levels[entry.depth] ??= []).push(entry);
    for (const child of entry.children) visitLevels(child);
  };
  visitLevels(root);
  const levelWidths = levels.map((entries) =>
    Math.max(...entries.map((entry) => topologyNodeSize(entry.node.icon).width)),
  );
  const widthWithoutGaps = levelWidths.reduce((total, width) => total + width, PADDING_X * 2);
  const gapCount = Math.max(0, levelWidths.length - 1);
  const levelGap = gapCount
    ? Math.min(
        MAX_LEVEL_GAP,
        Math.max(MIN_LEVEL_GAP, (availableWidth - widthWithoutGaps) / gapCount),
      )
    : 0;
  const levelX: number[] = [];
  let nextX = PADDING_X;
  for (let depth = 0; depth < levelWidths.length; depth += 1) {
    levelX.push(nextX);
    nextX += levelWidths[depth] + levelGap;
  }
  const subtreeHeights = new Map<string, number>();
  const measure = (entry: VisibleTopologyNode): number => {
    const ownHeight = topologyNodeSize(entry.node.icon).height;
    if (entry.children.length === 0) {
      subtreeHeights.set(entry.node.id, ownHeight);
      return ownHeight;
    }
    const childHeight =
      entry.children.reduce((total, child) => total + measure(child), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    const height = Math.max(ownHeight, childHeight);
    subtreeHeights.set(entry.node.id, height);
    return height;
  };
  const rootHeight = measure(root);
  const nodes: TopologyLayoutNode[] = [];
  const place = (entry: VisibleTopologyNode, top: number) => {
    const size = topologyNodeSize(entry.node.icon);
    const subtreeHeight = subtreeHeights.get(entry.node.id) ?? size.height;
    nodes.push({
      ...entry,
      x: levelX[entry.depth],
      y: top + (subtreeHeight - size.height) / 2,
      width: size.width,
      height: size.height,
      kind: size.kind,
    });
    if (entry.children.length === 0) return;
    const childrenHeight =
      entry.children.reduce((total, child) => total + (subtreeHeights.get(child.node.id) ?? 0), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    let childTop = top + (subtreeHeight - childrenHeight) / 2;
    for (const child of entry.children) {
      place(child, childTop);
      childTop += (subtreeHeights.get(child.node.id) ?? 0) + SUBTREE_GAP;
    }
  };
  place(root, PADDING_Y);
  const nodeById = new Map(nodes.map((node) => [node.node.id, node]));
  const edges = nodes.flatMap((child) => {
    if (!child.node.parentId) return [];
    const parent = nodeById.get(child.node.parentId);
    if (!parent) return [];
    const startX = parent.x + parent.width;
    const startY = parent.y + parent.height / 2;
    const endX = child.x;
    const endY = child.y + child.height / 2;
    const curve = Math.max(28, (endX - startX) / 2);
    return [
      {
        id: `${parent.node.id}->${child.node.id}`,
        parentId: parent.node.id,
        childId: child.node.id,
        path: `M ${startX} ${startY} C ${startX + curve} ${startY}, ${endX - curve} ${endY}, ${endX} ${endY}`,
        tone: child.node.tone,
      },
    ];
  });
  const contentWidth = nextX - levelGap + PADDING_X;
  return {
    width: Math.max(availableWidth, contentWidth),
    height: Math.max(420, rootHeight + PADDING_Y * 2),
    nodes,
    edges,
  };
}
export function topologyNodeSize(icon: TreeIcon): {
  width: number;
  height: number;
  kind: TopologyNodeKind;
} {
  if (["service", "host", "tenant", "codex", "claude"].includes(icon)) {
    return { width: 184, height: 58, kind: "entity" };
  }
  if (["config", "session-summary", "component"].includes(icon)) {
    return { width: 160, height: 38, kind: "leaf" };
  }
  return { width: 168, height: 46, kind: "group" };
}
export function topologyPath(root: TopologyNode, target: string): Set<string> {
  const path = new Set<string>();
  const visit = (node: TopologyNode): boolean => {
    if (node.id === target) {
      path.add(node.id);
      return true;
    }
    if (node.children.some(visit)) {
      path.add(node.id);
      return true;
    }
    return false;
  };
  visit(root);
  return path;
}
export function buildTopologyTree(
  data: TopologyData,
  sessions: Record<string, SessionLoad>,
): TopologyNode {
  const tenants = orderTenants(data.tenants).map((tenant) => tenantNode(tenant, sessions));
  return {
    id: "service",
    parentId: null,
    label: "aibox Service",
    detail: `${tenants.length} Tenants`,
    icon: "service",
    tone: maxTone(tenants.map((tenant) => tenant.tone)),
    children: tenants,
  };
}
export function tenantNode(
  row: TopologyTenant,
  sessions: Record<string, SessionLoad>,
): TopologyNode {
  const id = tenantId(row);
  const tenant = tenantSelection(row);
  const agents = (["codex", "claude"] as const)
    .map((agent) => row.agents.find((entry) => entry.agent === agent))
    .filter((agent): agent is TopologyAgent => Boolean(agent))
    .map((agent) => agentNode(id, tenant, agent, sessions));
  const components = componentNode(id, tenant, row.components.entries, row.components.error);
  const children = [...agents, components];
  return {
    id,
    parentId: "service",
    label: row.display_name,
    detail: row.home,
    title: row.home,
    icon: row.kind === "host" ? "host" : "tenant",
    tone: row.exists ? maxTone(children.map((child) => child.tone)) : "warning",
    target: { module: "tenants", query: tenantLocation(tenant) },
    children,
  };
}
export function agentNode(
  tenantIdValue: string,
  tenant: TenantSelection,
  agent: TopologyAgent,
  sessions: Record<string, SessionLoad>,
): TopologyNode {
  const id = `${tenantIdValue}/agent:${agent.agent}`;
  const configParams = tenantLocation(tenant);
  configParams.set("agent", agent.agent);
  configParams.set("current", "1");
  const currentTone: Tone = agent.current_config.error ? "error" : "neutral";
  const current: TopologyNode = {
    id: `${id}/current`,
    parentId: id,
    label: "Current Config",
    detail: agent.current_config.error
      ? "Inspection failed"
      : `${agent.current_config.present_files}/${agent.current_config.expected_files} files present`,
    title: agent.current_config.error,
    icon: "current",
    tone: currentTone,
    target: { module: "configs", query: configParams },
    children: [],
  };
  const namedId = `${id}/named-configs`;
  const namedChildren = agent.named_configs.entries.map((entry) => {
    const params = tenantLocation(tenant);
    params.set("agent", agent.agent);
    params.set("config", entry.name);
    const last = agent.application.last_application?.applied === entry.name;
    return {
      id: `${namedId}/${entry.name}`,
      parentId: namedId,
      label: entry.name,
      detail: `${capitalize(entry.state)}${last ? ` · Last applied ${formatTimestamp(agent.application.last_application!.applied_at)}` : ""}`,
      title: entry.detail,
      icon: "config" as const,
      tone:
        entry.state === "invalid"
          ? ("error" as const)
          : entry.state === "incomplete"
            ? ("warning" as const)
            : ("good" as const),
      target: { module: "configs" as const, query: params },
      children: [],
    };
  });
  const namedParams = tenantLocation(tenant);
  namedParams.set("agent", agent.agent);
  const named: TopologyNode = {
    id: namedId,
    parentId: id,
    label: "Named Configs",
    detail: agent.named_configs.error
      ? "Catalog inspection failed"
      : `${namedChildren.length} Configs`,
    title: agent.named_configs.error,
    icon: "configs",
    tone: agent.named_configs.error ? "error" : maxTone(namedChildren.map((node) => node.tone)),
    target: { module: "configs", query: namedParams },
    children: namedChildren,
  };
  const sessionId = `${id}/sessions`;
  const sessionLoad = sessions[sessionId];
  const sessionParams = new URLSearchParams();
  sessionParams.append("tenant", tenantSelectionValue(tenant));
  sessionParams.append("agent", agent.agent);
  const sessionChildren: TopologyNode[] = sessionLoad
    ? [sessionSummaryNode(sessionId, sessionLoad)]
    : [];
  const sessionTone =
    sessionLoad?.state === "error" || sessionLoad?.data?.partial ? "warning" : "neutral";
  const sessionNode: TopologyNode = {
    id: sessionId,
    parentId: id,
    label: "Sessions",
    detail: sessionLoadDetail(sessionLoad),
    title: sessionLoad?.error ?? sessionLoad?.data?.warnings.join("\n"),
    icon: "sessions",
    tone: sessionTone,
    target: { module: "sessions", query: sessionParams },
    sessionRequest: { tenant, agent: agent.agent },
    children: sessionChildren,
  };
  const children = [current, named, sessionNode];
  const driftTone = configDriftTone(agent.application.drift);
  const last = agent.application.last_application;
  const drift = humanDrift(agent.application.drift);
  return {
    id,
    parentId: tenantIdValue,
    label: agent.agent === "codex" ? "Codex" : "Claude",
    detail: last ? `Last applied ${last.applied} · ${drift}` : `Config Drift ${drift}`,
    title: agent.application.detail,
    icon: agent.agent,
    tone: maxTone([driftTone, ...children.map((child) => child.tone)]),
    target: { module: "configs", query: configParams },
    children,
  };
}
export function componentNode(
  tenantIdValue: string,
  tenant: TenantSelection,
  entries: ComponentRow[],
  error?: string,
): TopologyNode {
  const id = `${tenantIdValue}/components`;
  const visible = entries.filter((entry) => entry.status !== "not-installed" || entry.error);
  const children = visible.map((entry) => {
    const params = tenantLocation(tenant);
    params.set("component", entry.kind);
    return {
      id: `${id}/${entry.kind}`,
      parentId: id,
      label: componentLabel(entry.kind),
      detail: componentDetail(entry),
      title: entry.error ?? undefined,
      icon: "component" as const,
      tone: componentTone(entry),
      target: { module: "tenants" as const, query: params },
      children: [],
    };
  });
  const installed = entries.filter((entry) => entry.status === "installed").length;
  const params = tenantLocation(tenant);
  return {
    id,
    parentId: tenantIdValue,
    label: "Components",
    detail: error ? "Catalog inspection failed" : `${installed}/${entries.length} installed`,
    title: error,
    icon: "components",
    tone: error ? "error" : maxTone(children.map((child) => child.tone)),
    target: { module: "tenants", query: params },
    children,
  };
}
export function sessionSummaryNode(parentId: string, load: SessionLoad): TopologyNode {
  if (load.state === "loading") {
    return {
      id: `${parentId}/summary`,
      parentId,
      label: "Discovering Transcripts",
      icon: "session-summary",
      tone: "neutral",
      children: [],
    };
  }
  if (load.state === "error") {
    return {
      id: `${parentId}/summary`,
      parentId,
      label: "Session summary unavailable",
      detail: load.error,
      title: load.error,
      icon: "session-summary",
      tone: "error",
      children: [],
    };
  }
  const data = load.data!;
  return {
    id: `${parentId}/summary`,
    parentId,
    label: `${data.count} ${data.count === 1 ? "Session" : "Sessions"}`,
    detail: data.partial ? `${data.warnings.length} traversal warnings` : "Discovery complete",
    title: data.warnings.join("\n") || undefined,
    icon: "session-summary",
    tone: data.partial ? "warning" : "good",
    children: [],
  };
}
export function attentionTenant(tenant: TopologyTenant): string {
  return tenant.kind === "host" ? "host" : `managed:${tenant.name}`;
}
export function configAttentionTarget(
  tenant: TopologyTenant,
  agent: TopologyAgent,
  config?: string,
): NavigationTarget {
  const query = new URLSearchParams();
  query.set("tenant", attentionTenant(tenant));
  query.set("agent", agent.agent);
  if (config) query.set("config", config);
  else query.set("current", "1");
  return { module: "configs", query };
}
export function firstConfigAttentionTarget(data: TopologyData): NavigationTarget {
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      if (
        agent.current_config.error ||
        ["dirty", "source-missing", "comparison-error"].includes(agent.application.drift)
      ) {
        return configAttentionTarget(tenant, agent);
      }
      const entry = agent.named_configs.entries.find(
        (candidate) => candidate.state === "incomplete" || candidate.state === "invalid",
      );
      if (entry) return configAttentionTarget(tenant, agent, entry.name);
      if (agent.named_configs.error) return configAttentionTarget(tenant, agent);
    }
  }
  return { module: "configs" };
}
export function firstComponentAttentionTarget(data: TopologyData): NavigationTarget {
  for (const tenant of orderTenants(data.tenants)) {
    const query = new URLSearchParams();
    query.set("tenant", attentionTenant(tenant));
    const entry = tenant.components.entries.find(
      (candidate) =>
        candidate.error || ["modified", "incomplete", "unmanaged"].includes(candidate.status ?? ""),
    );
    if (entry) query.set("component", entry.kind);
    if (entry || tenant.components.error) return { module: "tenants", query };
  }
  return { module: "tenants" };
}
export function summarizeTopology(data: TopologyData): TopologyHealth {
  const summary: TopologyHealth = {
    configTotal: 0,
    configAttention: 0,
    configErrors: 0,
    componentInstalled: 0,
    componentAttention: 0,
    componentErrors: 0,
  };
  for (const tenant of data.tenants) {
    for (const agent of tenant.agents) {
      summary.configTotal += agent.named_configs.entries.length;
      if (agent.current_config.error) {
        summary.configAttention += 1;
        summary.configErrors += 1;
      }
      if (["dirty", "source-missing", "comparison-error"].includes(agent.application.drift)) {
        summary.configAttention += 1;
        if (agent.application.drift === "comparison-error") summary.configErrors += 1;
      }
      if (agent.named_configs.error) {
        summary.configAttention += 1;
        summary.configErrors += 1;
      }
      for (const entry of agent.named_configs.entries) {
        if (entry.state === "incomplete" || entry.state === "invalid") summary.configAttention += 1;
        if (entry.state === "invalid") summary.configErrors += 1;
      }
    }
    if (tenant.components.error) {
      summary.componentAttention += 1;
      summary.componentErrors += 1;
    }
    for (const entry of tenant.components.entries) {
      if (entry.status === "installed") summary.componentInstalled += 1;
      if (entry.error || ["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) {
        summary.componentAttention += 1;
      }
      if (entry.error) summary.componentErrors += 1;
    }
  }
  return summary;
}
export function orderTenants(tenants: TopologyTenant[]): TopologyTenant[] {
  const defaultTenant = tenants.find(
    (tenant) => tenant.kind === "managed" && tenant.name === "default",
  );
  const host = tenants.find((tenant) => tenant.kind === "host");
  const rest = tenants
    .filter((tenant) => tenant !== defaultTenant && tenant !== host)
    .sort((left, right) => left.display_name.localeCompare(right.display_name));
  return [host, defaultTenant, ...rest].filter((tenant): tenant is TopologyTenant =>
    Boolean(tenant),
  );
}
export function tenantId(tenant: TopologyTenant): string {
  return tenant.kind === "host" ? "tenant:host" : `tenant:managed:${tenant.name}`;
}
export function tenantSelection(tenant: TopologyTenant): TenantSelection {
  return tenant.kind === "host" ? { kind: "host" } : { kind: "managed", name: tenant.name };
}
export function tenantLocation(tenant: TenantSelection): URLSearchParams {
  return new URLSearchParams({ tenant: tenantSelectionValue(tenant) });
}
export function structuralIds(data: TopologyData): Set<string> {
  const ids = new Set<string>();
  for (const tenant of data.tenants) {
    const base = tenantId(tenant);
    ids.add(base);
    for (const agent of tenant.agents) {
      const agentId = `${base}/agent:${agent.agent}`;
      ids.add(agentId);
      if (agent.named_configs.entries.length) ids.add(`${agentId}/named-configs`);
    }
    if (
      tenant.components.entries.some((entry) => entry.status !== "not-installed" || entry.error)
    ) {
      ids.add(`${base}/components`);
    }
  }
  return ids;
}
export function defaultExpansion(data: TopologyData): Set<string> {
  return structuralIds(data);
}
export function collectVisibleNodes(
  root: TopologyNode,
  expanded: Set<string>,
  forced: Set<string>,
  parentId: string | null = null,
): Array<{
  node: TopologyNode;
  parentId: string | null;
}> {
  const result = [{ node: root, parentId }];
  const open = root.id === "service" || expanded.has(root.id) || forced.has(root.id);
  if (open) {
    for (const child of root.children)
      result.push(...collectVisibleNodes(child, expanded, forced, root.id));
  }
  return result;
}
export function collectBranchIds(node: TopologyNode, result: Set<string>) {
  if (node.children.length) result.add(node.id);
  for (const child of node.children) collectBranchIds(child, result);
}
export function searchTopology(root: TopologyNode | null, query: string): TopologySearchResult {
  const matches = new Set<string>();
  const context = new Set<string>();
  const normalized = query.toLocaleLowerCase();
  let firstMatch: string | null = null;
  if (!root || !normalized) return { matches, context, firstMatch };
  const visit = (node: TopologyNode, ancestors: string[]) => {
    const text = `${node.label} ${node.detail ?? ""} ${node.title ?? ""}`.toLocaleLowerCase();
    if (text.includes(normalized)) {
      matches.add(node.id);
      firstMatch ??= node.id;
      for (const id of ancestors) context.add(id);
    }
    for (const child of node.children) visit(child, [...ancestors, node.id]);
  };
  visit(root, []);
  return { matches, context, firstMatch };
}
export function filterByAttention(node: TopologyNode): TopologyNode | null {
  const children = node.children
    .map(filterByAttention)
    .filter((child): child is TopologyNode => Boolean(child));
  const matches = node.tone === "warning" || node.tone === "error";
  return matches || children.length ? { ...node, children } : null;
}
export function emptyFilteredRoot(root: TopologyNode, detail: string): TopologyNode {
  return { ...root, detail, tone: "good", children: [] };
}
export function targetHref(target: NavigationTarget): string {
  const query = target.query?.toString();
  return `/_aibox/ui/${target.module}${query ? `?${query}` : ""}`;
}
export function maxTone(tones: Tone[]): Tone {
  if (tones.includes("error")) return "error";
  if (tones.includes("warning")) return "warning";
  if (tones.includes("good")) return "good";
  return "neutral";
}
export function configDriftTone(drift: string): Tone {
  if (drift === "comparison-error") return "error";
  if (drift === "dirty" || drift === "source-missing") return "warning";
  if (drift === "clean") return "good";
  return "neutral";
}
export function humanDrift(drift: string): string {
  return drift.split("-").map(capitalize).join(" ");
}
export function componentTone(entry: ComponentRow): Tone {
  if (entry.error) return "error";
  if (["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) return "warning";
  if (entry.status === "installed") return "good";
  return "neutral";
}
export function componentLabel(kind: string): string {
  return kind.split("-").map(capitalize).join(" ");
}
export function componentDetail(entry: ComponentRow): string {
  if (entry.error) return "Inspection failed";
  const status = entry.status ? capitalize(entry.status) : "Unknown";
  return entry.version ? `${status} · ${entry.version}` : status;
}
export function sessionLoadDetail(load?: SessionLoad): string {
  if (!load) return "Load count on demand";
  if (load.state === "loading") return "Discovering Transcripts";
  if (load.state === "error") return "Summary unavailable";
  return `${load.data!.count} Sessions${load.data!.partial ? " · Partial" : ""}`;
}
export function attentionValue(value: number): string {
  return value === 0 ? "Healthy" : `${value} need attention`;
}
export function healthTone(attention?: number, errors?: number, loadError?: string | null): Tone {
  if (loadError || errors) return "error";
  if (attention) return "warning";
  return attention === 0 ? "good" : "neutral";
}
export function requestDetail(data: OverviewData): string {
  const states = [
    data.requests.active ? `${data.requests.active} active` : "",
    data.requests.warning ? `${data.requests.warning} warning` : "",
    data.requests.error ? `${data.requests.error} error` : "",
    formatBytes(data.requests.bytes),
  ].filter(Boolean);
  return states.join(" · ");
}
export function requestAttentionDetail(data: OverviewData): string {
  const errorLabel = `${data.requests.error} error${data.requests.error === 1 ? "" : "s"}`;
  const warningLabel = `${data.requests.warning} warning${data.requests.warning === 1 ? "" : "s"}`;
  return `${errorLabel} · ${warningLabel}`;
}
export function imageTone(status?: OverviewData["runtime_image"]["status"]): Tone {
  if (status === "built") return "good";
  if (status === "missing") return "warning";
  return "neutral";
}
export function shortImageId(id: string | null | undefined): string {
  if (!id) return "—";
  const value = id.startsWith("sha256:") ? id.slice(7) : id;
  return value.slice(0, 12);
}
export function buildDisabledReason(
  data: OverviewData | null,
  operation: Operation | null,
): string {
  if (operation?.state === "running") return `Unavailable while ${operation.kind} is running`;
  if (data?.docker.status === "unavailable") return data.docker.error ?? "Docker is unavailable";
  return "Status is still loading";
}
export function formatDuration(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  if (days) return `${days}d ${hours}h`;
  if (hours) return `${hours}h ${minutes}m`;
  if (minutes) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}
export function capitalize(value: string): string {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}
