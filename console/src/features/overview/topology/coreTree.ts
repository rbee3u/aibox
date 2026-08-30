import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import type { SessionSummaryData } from "@/api/sessions";
import type { ComponentRow } from "@/api/tenants";
import { tenantSelectionValue, type TenantSelection } from "@/domain/tenant";
import type { ModuleId } from "@/shared/lib/navigation";
import { formatTimestamp } from "@/shared/lib/format";

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
): TopologyNode {
  const tenants = orderTenants(data.tenants).map((tenant) => tenantNode(tenant, sessions));
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
    return {
      id: `${id}/${entry.kind}`,
      parentId: id,
      label: componentLabel(entry.kind),
      detail: componentDetail(entry),
      title: entry.error ?? undefined,
      icon: "component" as const,
      tone: componentTone(entry),
      target: { module: "tenants" as const, query: tenantLocation(tenant) },
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
export function capitalize(value: string): string {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}
