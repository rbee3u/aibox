import type { CodingAgentKind } from "@/domain/codingAgent";
import type {
  TopologyAgent,
  TopologyComponents,
  TopologyData,
  TopologyTenant,
} from "@/api/overview";
import type { SessionSummaryData } from "@/api/sessions";
import type { ComponentRow } from "@/api/tenants";
import { tenantSelectionValue, type TenantSelection } from "@/domain/tenant";
import type { ModuleId } from "@/shared/lib/navigation";
import { capitalize, formatTimestamp } from "@/shared/lib/format";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";
import type { Tone } from "@/features/overview/viewTypes";

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
export interface InspectorFact {
  label: string;
  value: string;
}
export interface TopologyNode {
  id: string;
  parentId: string | null;
  label: string;
  detail?: string;
  title?: string;
  tooltip?: string;
  facts?: InspectorFact[];
  icon: TreeIcon;
  tone: Tone;
  target?: NavigationTarget;
  sessionRequest?: SessionRequest;
  children: TopologyNode[];
}
export interface SessionLoad {
  state: "loading" | "loaded" | "error";
  data?: SessionSummaryData;
  error?: string;
}
export function sessionAnnouncement(loads: Record<string, SessionLoad>): string {
  const loading = Object.values(loads).filter((load) => load.state === "loading").length;
  if (loading) return `Discovering ${loading} Session ${loading === 1 ? "summary" : "summaries"}`;
  const latest = Object.values(loads).at(-1);
  if (!latest) return "";
  if (latest.state === "error") return "Session summary unavailable";
  return latest.data ? `${latest.data.count} Sessions discovered` : "";
}
export function buildTopologyTree(
  data: TopologyData,
  sessions: Record<string, SessionLoad>,
  hostHome: string | null,
): TopologyNode {
  const tenants = orderTenants(data.tenants).map((tenant) =>
    tenantNode(tenant, sessions, hostHome),
  );
  return {
    id: "service",
    parentId: null,
    label: "AIBox Service",
    detail: `${tenants.length} Tenants`,
    icon: "service",
    tone: maxTone(tenants.map((tenant) => tenant.tone)),
    children: tenants,
  };
}
export function tenantNode(
  row: TopologyTenant,
  sessions: Record<string, SessionLoad>,
  hostHome: string | null,
): TopologyNode {
  const id = tenantId(row);
  const tenant = tenantSelection(row);
  const agents = (["codex", "claude"] as const)
    .map((agent) => row.agents.find((entry) => entry.agent === agent))
    .filter((agent): agent is TopologyAgent => Boolean(agent))
    .map((agent) => agentNode(id, tenant, agent, sessions));
  const components = componentNode(id, tenant, row.components);
  const children = [...agents, components];
  return {
    id,
    parentId: "service",
    label: row.display_name,
    detail: abbreviateTenantHome(row.home, hostHome),
    tooltip: row.home,
    facts: tenantFacts(row),
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
  const current: TopologyNode = {
    id: `${id}/current`,
    parentId: id,
    label: "Current Config",
    detail: currentConfigDetail(agent),
    title: currentConfigTitle(agent),
    facts: currentConfigFacts(agent),
    icon: "current",
    tone: currentConfigTone(agent),
    target: { module: "configs", query: configParams },
    children: [],
  };
  const namedId = `${id}/named-configs`;
  const namedChildren = agent.named_configs.attention.map((entry) => {
    const params = tenantLocation(tenant);
    params.set("agent", agent.agent);
    params.set("config", entry.name);
    const last = agent.application.last_application?.applied === entry.name;
    const detail = `${capitalize(entry.state)}${last ? ` · Last applied ${formatTimestamp(agent.application.last_application!.applied_at)}` : ""}`;
    return {
      id: `${namedId}/${entry.name}`,
      parentId: namedId,
      label: entry.name,
      detail,
      tooltip: last ? detail : undefined,
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
  const named: TopologyNode = {
    id: namedId,
    parentId: id,
    label: "Named Configs",
    detail: agent.named_configs.error
      ? "Catalog inspection failed"
      : `${agent.named_configs.count} Configs`,
    title: agent.named_configs.error,
    facts: namedConfigFacts(agent),
    icon: "configs",
    tone: agent.named_configs.error ? "error" : maxTone(namedChildren.map((node) => node.tone)),
    target: { module: "configs", query: namedCatalogLocation(tenant, agent.agent) },
    children: namedChildren,
  };
  const sessionId = `${id}/sessions`;
  const sessionLoad = sessions[sessionId];
  const sessionParams = new URLSearchParams();
  sessionParams.append("tenant", tenantSelectionValue(tenant));
  sessionParams.append("agent", agent.agent);
  const sessionTone =
    sessionLoad?.state === "error" || sessionLoad?.data?.partial ? "warning" : "neutral";
  const sessionNode: TopologyNode = {
    id: sessionId,
    parentId: id,
    label: "Sessions",
    detail: sessionLoadDetail(sessionLoad),
    tooltip: sessionLoadTooltip(sessionLoad),
    title: sessionLoad?.error ?? sessionLoad?.data?.warnings.join("\n"),
    facts: sessionFacts(sessionLoad),
    icon: "sessions",
    tone: sessionTone,
    target: { module: "sessions", query: sessionParams },
    sessionRequest: { tenant, agent: agent.agent },
    children: [],
  };
  const children = [current, named, sessionNode];
  const driftTone = configDriftTone(agent.application.drift);
  return {
    id,
    parentId: tenantIdValue,
    label: agent.agent === "codex" ? "Codex" : "Claude",
    detail: agentCardDetail(agent),
    tooltip: agentCardTooltip(agent),
    title: agent.application.detail,
    facts: agentFacts(agent),
    icon: agent.agent,
    tone: maxTone([driftTone, ...children.map((child) => child.tone)]),
    target: { module: "configs", query: configParams },
    children,
  };
}
export function componentNode(
  tenantIdValue: string,
  tenant: TenantSelection,
  summary: TopologyComponents,
): TopologyNode {
  const id = `${tenantIdValue}/components`;
  const children = summary.attention.map((entry) => {
    return {
      id: `${id}/${entry.kind}`,
      parentId: id,
      label: componentLabel(entry.kind),
      detail: componentDetail(entry),
      title: entry.error ?? undefined,
      icon: "component" as const,
      tone: componentTone(entry),
      target: { module: "tenants" as const, query: tenantComponentLocation(tenant, entry.kind) },
      children: [],
    };
  });
  const params = tenantLocation(tenant);
  return {
    id,
    parentId: tenantIdValue,
    label: "Components",
    detail: summary.error
      ? "Catalog inspection failed"
      : `${summary.installed}/${summary.total} installed`,
    title: summary.error,
    icon: "components",
    tone: summary.error ? "error" : maxTone(children.map((child) => child.tone)),
    target: { module: "tenants", query: params },
    children,
  };
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
export function tenantComponentLocation(
  tenant: TenantSelection,
  kind: ComponentRow["kind"],
): URLSearchParams {
  const query = tenantLocation(tenant);
  query.set("component", kind);
  return query;
}
export function namedCatalogLocation(
  tenant: TenantSelection,
  agent: CodingAgentKind,
): URLSearchParams {
  const query = tenantLocation(tenant);
  query.set("agent", agent);
  query.set("named", "1");
  return query;
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
export function currentConfigTone(agent: TopologyAgent): Tone {
  if (agent.current_config.error) return "error";
  const driftTone = configDriftTone(agent.application.drift);
  return driftTone === "warning" || driftTone === "error" ? driftTone : "neutral";
}
export function currentConfigDetail(agent: TopologyAgent): string {
  if (agent.current_config.error) return "Inspection failed";
  if (currentConfigTone(agent) !== "neutral") return humanDrift(agent.application.drift);
  return `${agent.current_config.present_files}/${agent.current_config.expected_files} files present`;
}
export function currentConfigTitle(agent: TopologyAgent): string | undefined {
  if (agent.current_config.error) return agent.current_config.error;
  if (currentConfigTone(agent) !== "neutral") return agent.application.detail;
  return undefined;
}
export function lastAppliedFact(agent: TopologyAgent): InspectorFact | undefined {
  const last = agent.application.last_application;
  if (!last) return undefined;
  return { label: "Last applied", value: `${last.applied} · ${formatTimestamp(last.applied_at)}` };
}
export function currentConfigFacts(agent: TopologyAgent): InspectorFact[] {
  const facts = collectFacts(lastAppliedFact(agent));
  if (agent.current_config.error) return facts;
  const files = `${agent.current_config.present_files}/${agent.current_config.expected_files} present`;
  if (
    currentConfigDetail(agent) !==
    `${agent.current_config.present_files}/${agent.current_config.expected_files} files present`
  ) {
    facts.push({ label: "Files", value: files });
  }
  return facts;
}
export function agentFacts(agent: TopologyAgent): InspectorFact[] {
  return collectFacts(lastAppliedFact(agent));
}
export function namedConfigFacts(agent: TopologyAgent): InspectorFact[] {
  if (agent.named_configs.error) return [];
  if (agent.named_configs.count > 0 && agent.named_configs.attention.length === 0) {
    return [{ label: "Attention", value: "None need attention" }];
  }
  return [];
}
export function sessionFacts(load?: SessionLoad): InspectorFact[] {
  if (!load) return [{ label: "Summary", value: "Load on demand" }];
  if (load.state === "loading") return [{ label: "Summary", value: "Discovering Transcripts" }];
  if (load.state === "loaded") return [{ label: "Summary", value: sessionLoadDetail(load) }];
  return [];
}
export function inspectorFacts(node: TopologyNode): InspectorFact[] {
  if (node.facts && node.facts.length > 0) return node.facts;
  if (!node.detail) return [];
  return [{ label: inspectorFallbackLabel(node.icon), value: node.detail }];
}
export function inspectorFallbackLabel(icon: TreeIcon): string {
  if (icon === "sessions") return "Summary";
  if (icon === "service") return "Tenants";
  if (icon === "host" || icon === "tenant") return "Home";
  return "Status";
}
export function tenantFacts(row: TopologyTenant): InspectorFact[] {
  return [{ label: "Home", value: row.home }];
}
function collectFacts(...entries: Array<InspectorFact | undefined>): InspectorFact[] {
  return entries.filter((entry): entry is InspectorFact => Boolean(entry));
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
export function agentCardDetail(agent: TopologyAgent): string {
  const last = agent.application.last_application;
  const drift = humanDrift(agent.application.drift);
  return last ? `${last.applied} · ${drift}` : drift;
}

export function agentCardTooltip(agent: TopologyAgent): string {
  const last = agent.application.last_application;
  const drift = humanDrift(agent.application.drift);
  return last ? `Last applied ${last.applied} · ${drift}` : `Config Drift ${drift}`;
}

export function sessionLoadDetail(load?: SessionLoad): string {
  if (!load) return "Load count";
  if (load.state === "loading") return "Discovering";
  if (load.state === "error") return "Unavailable";
  return `${load.data!.count} Sessions${load.data!.partial ? " · Partial" : ""}`;
}

export function sessionLoadTooltip(load?: SessionLoad): string | undefined {
  if (!load) return "Load count on demand";
  if (load.state === "loading") return "Discovering Transcripts";
  if (load.state === "error") return load.error ?? "Session summary unavailable";
  return undefined;
}

/** Full text for hover and keyboard focus when the card line is shortened or truncated. */
export function topologyNodeDisclosure(node: TopologyNode): string | undefined {
  if (node.tooltip) {
    return node.title && node.title !== node.tooltip
      ? `${node.tooltip}\n${node.title}`
      : node.tooltip;
  }
  if (node.title && node.title !== node.detail) return node.title;
  return undefined;
}

export function findTopologyNode(root: TopologyNode | null, id: string): TopologyNode | null {
  if (!root) return null;
  if (root.id === id) return root;
  for (const child of root.children) {
    const match = findTopologyNode(child, id);
    if (match) return match;
  }
  return null;
}

export function structuralIds(data: TopologyData): Set<string> {
  const ids = new Set<string>();
  for (const tenant of data.tenants) {
    const base = tenantId(tenant);
    ids.add(base);
    for (const agent of tenant.agents) {
      const agentId = `${base}/agent:${agent.agent}`;
      ids.add(agentId);
      ids.add(`${agentId}/named-configs`);
    }
    ids.add(`${base}/components`);
  }
  return ids;
}

export function attentionCountLabel(count: number): string {
  return `${count} ${count === 1 ? "needs" : "need"} attention`;
}

export function healthyDefaultExpansion(data: TopologyData): Set<string> {
  const defaultTenant = data.tenants.find(
    (tenant) => tenant.kind === "managed" && tenant.name === "default",
  );
  const fallback = defaultTenant ?? data.tenants.find((tenant) => tenant.kind === "host");
  if (!fallback) return new Set();
  const base = tenantId(fallback);
  return new Set([base, ...fallback.agents.map((agent) => `${base}/agent:${agent.agent}`)]);
}
