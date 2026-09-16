import type { CodingAgentKind } from "@/domain/codingAgent";
import type {
  TopologyAgent,
  TopologyComponents,
  TopologyData,
  TopologyTenant,
} from "@/api/overview";
import type { ComponentRow } from "@/api/tenants";
import { tenantSelectionValue, type TenantSelection } from "@/domain/tenant";
import type { ModuleId } from "@/shared/lib/navigation";
import { capitalize, driftCatalogLabel } from "@/shared/lib/format";
import type { Tone } from "@/features/overview/viewTypes";

export type TreeIcon =
  | "service"
  | "host"
  | "tenant"
  | "codex"
  | "claude"
  | "current"
  | "configs"
  | "sessions"
  | "components";
export interface NavigationTarget {
  module: ModuleId;
  query?: URLSearchParams;
}
export interface AttentionItem {
  /** Stable identity: several items now share one category label. */
  key: string;
  label: string;
  detail: string;
  /**
   * The raw diagnostic behind `detail`, when the row states a cause the
   * Console worded itself. Kept as evidence behind a disclosure so the
   * sentence stays readable without discarding what the Service reported.
   */
  technical?: string;
  tone: "warning" | "error";
  target?: NavigationTarget;
  retry?: "service" | "topology";
}
export interface TopologyNode {
  id: string;
  parentId: string | null;
  label: string;
  detail?: string;
  icon: TreeIcon;
  tone: Tone;
  target?: NavigationTarget;
  children: TopologyNode[];
}

/** The Service root provides identity; health remains in the status strip. */
export function buildTopologyTree(data: TopologyData): TopologyNode {
  return {
    id: "service",
    parentId: null,
    label: "AIBox Service",
    icon: "service",
    tone: "neutral",
    children: orderTenants(data.tenants).map(tenantNode),
  };
}
function tenantNode(row: TopologyTenant): TopologyNode {
  const id = tenantId(row);
  const tenant = tenantSelection(row);
  const children = [
    ...(["codex", "claude"] as const).flatMap((kind) => {
      const agent = row.agents.find((entry) => entry.agent === kind);
      return agent ? [agentNode(id, tenant, agent)] : [];
    }),
    componentNode(id, tenant, row.components),
  ];
  return {
    id,
    parentId: "service",
    label: row.display_name,
    detail: row.exists ? undefined : "Home unavailable",
    icon: row.kind === "host" ? "host" : "tenant",
    tone: row.exists ? maxTone(children.map((child) => child.tone)) : "warning",
    target: { module: "tenants", query: tenantLocation(tenant) },
    children,
  };
}
function agentNode(parentId: string, tenant: TenantSelection, agent: TopologyAgent): TopologyNode {
  const id = `${parentId}/agent:${agent.agent}`;
  const currentQuery = tenantLocation(tenant);
  currentQuery.set("agent", agent.agent);
  currentQuery.set("current", "1");
  const currentTone = currentConfigTone(agent);
  const namedAttention = agent.named_configs.attention.filter(
    (entry) => entry.state === "invalid" || entry.state === "incomplete",
  );
  const namedTone =
    agent.named_configs.error || namedAttention.some((entry) => entry.state === "invalid")
      ? "error"
      : namedAttention.length
        ? "warning"
        : "neutral";
  const sessionQuery = tenantLocation(tenant);
  sessionQuery.set("agent", agent.agent);
  const children: TopologyNode[] = [
    {
      id: `${id}/current`,
      parentId: id,
      label: "Current Config",
      icon: "current",
      detail: agent.current_config.error
        ? "Inspection failed"
        : currentTone === "neutral"
          ? undefined
          : driftCatalogLabel(agent.application.drift),
      tone: currentTone,
      target: { module: "configs", query: currentQuery },
      children: [],
    },
    {
      id: `${id}/named-configs`,
      parentId: id,
      label: "Named Configs",
      icon: "configs",
      detail: agent.named_configs.error
        ? "Catalog inspection failed"
        : `${agent.named_configs.count} Named Configs${namedAttention.length ? ` · ${attentionCountLabel(namedAttention.length)}` : ""}`,
      tone: namedTone,
      target: { module: "configs", query: namedCatalogLocation(tenant, agent.agent) },
      children: [],
    },
    {
      id: `${id}/sessions`,
      parentId: id,
      label: "Sessions",
      icon: "sessions",
      detail: agent.sessions.error
        ? "Session inspection failed"
        : `${agent.sessions.count} Sessions`,
      tone: agent.sessions.error ? "error" : "neutral",
      target: { module: "sessions", query: sessionQuery },
      children: [],
    },
  ];
  return {
    id,
    parentId,
    label: agent.agent === "codex" ? "Codex" : "Claude",
    icon: agent.agent,
    detail: agent.application.last_application
      ? `Last applied: ${agent.application.last_application.applied}`
      : undefined,
    tone: maxTone(children.map((child) => child.tone)),
    children,
  };
}
function componentNode(
  parentId: string,
  tenant: TenantSelection,
  summary: TopologyComponents,
): TopologyNode {
  const attention = summary.attention.filter((entry) =>
    ["warning", "error"].includes(componentTone(entry)),
  );
  return {
    id: `${parentId}/components`,
    parentId,
    label: "Components",
    icon: "components",
    detail: summary.error
      ? "Catalog inspection failed"
      : `${summary.installed}/${summary.total} installed${attention.length ? ` · ${attentionCountLabel(attention.length)}` : ""}`,
    tone: summary.error ? "error" : maxTone(attention.map(componentTone)),
    target: { module: "tenants", query: tenantLocation(tenant) },
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
export function currentConfigTone(agent: TopologyAgent): Tone {
  if (agent.current_config.error) return "error";
  const driftTone = configDriftTone(agent.application.drift);
  return driftTone === "warning" || driftTone === "error" ? driftTone : "neutral";
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

export function attentionCountLabel(count: number): string {
  return `${count} ${count === 1 ? "needs" : "need"} attention`;
}
