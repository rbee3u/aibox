import type { Operation } from "@/api/operations";
import type { OverviewData, TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import type { ComponentKind } from "@/api/tenants";
import {
  attentionCountLabel,
  componentLabel,
  healthyDefaultExpansion,
  namedCatalogLocation,
  orderTenants,
  tenantComponentLocation,
  tenantId,
  tenantLocation,
  tenantSelection,
  type AttentionItem,
  type NavigationTarget,
} from "@/features/overview/topology/coreTree";
import type { Tone } from "@/features/overview/viewTypes";

export interface TopologyHealth {
  configTotal: number;
  configAttention: number;
  configErrors: number;
  componentInstalled: number;
  componentAttention: number;
  componentErrors: number;
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
export interface AttentionTarget {
  target: NavigationTarget;
  subject: string;
}

function agentTitle(agent: TopologyAgent): string {
  return agent.agent === "codex" ? "Codex" : "Claude";
}

function attentionScope(tenant: TopologyTenant, agent?: TopologyAgent): string {
  return agent ? `${tenant.display_name} · ${agentTitle(agent)}` : tenant.display_name;
}

export function attentionTargetDetail(subject: string, total: number): string {
  return total > 1 ? `${subject} · +${total - 1} more` : subject;
}

type NamedAttentionState = "incomplete" | "invalid";

type ConfigAttentionHit =
  | { tenant: TopologyTenant; agent: TopologyAgent; kind: "current" }
  | {
      tenant: TopologyTenant;
      agent: TopologyAgent;
      kind: "named";
      name: string;
      state: NamedAttentionState;
    }
  | { tenant: TopologyTenant; agent: TopologyAgent; kind: "catalog" };

function currentConfigReason(agent: TopologyAgent): string {
  if (agent.current_config.error) return "Current Config inspection failed";
  if (agent.application.drift === "dirty") return "Current Config is dirty";
  if (agent.application.drift === "source-missing") return "Current Config source is missing";
  if (agent.application.drift === "comparison-error") return "Current Config comparison failed";
  return "Current Config";
}

function namedConfigReason(name: string, state: NamedAttentionState): string {
  return state === "invalid" ? `${name} is invalid` : `${name} is incomplete`;
}

function firstConfigAttentionHit(data: TopologyData): ConfigAttentionHit | null {
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      if (
        agent.current_config.error ||
        ["dirty", "source-missing", "comparison-error"].includes(agent.application.drift)
      ) {
        return { tenant, agent, kind: "current" };
      }
      const entry = agent.named_configs.attention.find(
        (candidate) => candidate.state === "incomplete" || candidate.state === "invalid",
      );
      if (entry && (entry.state === "incomplete" || entry.state === "invalid")) {
        return { tenant, agent, kind: "named", name: entry.name, state: entry.state };
      }
      if (agent.named_configs.error) return { tenant, agent, kind: "catalog" };
    }
  }
  return null;
}

export function firstConfigAttention(data: TopologyData): AttentionTarget {
  const hit = firstConfigAttentionHit(data);
  if (!hit) return { target: { module: "configs" }, subject: "Named Configs" };
  const scope = attentionScope(hit.tenant, hit.agent);
  if (hit.kind === "named") {
    return {
      target: configAttentionTarget(hit.tenant, hit.agent, hit.name),
      subject: `${scope} · ${namedConfigReason(hit.name, hit.state)}`,
    };
  }
  if (hit.kind === "catalog") {
    return {
      target: {
        module: "configs",
        query: namedCatalogLocation(tenantSelection(hit.tenant), hit.agent.agent),
      },
      subject: `${scope} · Named Configs inspection failed`,
    };
  }
  return {
    target: configAttentionTarget(hit.tenant, hit.agent),
    subject: `${scope} · ${currentConfigReason(hit.agent)}`,
  };
}

export function firstConfigAttentionTarget(data: TopologyData): NavigationTarget {
  return firstConfigAttention(data).target;
}

export function configAttentionItem(data: TopologyData, health: TopologyHealth): AttentionItem {
  const { target, subject } = firstConfigAttention(data);
  return {
    label: "Configs",
    detail: attentionTargetDetail(subject, health.configAttention),
    tone: health.configErrors ? "error" : "warning",
    target,
  };
}

function componentAttentionReason(input: {
  kind: ComponentKind | null;
  status?: string | null;
  error?: string | null;
}): string {
  if (!input.kind) return "Components inspection failed";
  const label = componentLabel(input.kind);
  if (input.error) return `${label} inspection failed`;
  if (input.status === "modified") return `${label} is modified`;
  if (input.status === "incomplete") return `${label} is incomplete`;
  if (input.status === "unmanaged") return `${label} is unmanaged`;
  return label;
}

function firstComponentAttentionHit(data: TopologyData): {
  tenant: TopologyTenant;
  kind: ComponentKind | null;
  status?: string | null;
  error?: string | null;
} | null {
  for (const tenant of orderTenants(data.tenants)) {
    const entry = tenant.components.attention.find(
      (candidate) =>
        candidate.error || ["modified", "incomplete", "unmanaged"].includes(candidate.status ?? ""),
    );
    if (entry) {
      return { tenant, kind: entry.kind, status: entry.status, error: entry.error };
    }
    if (tenant.components.error) return { tenant, kind: null, error: tenant.components.error };
  }
  return null;
}

export function firstComponentAttention(data: TopologyData): AttentionTarget {
  const hit = firstComponentAttentionHit(data);
  if (!hit) return { target: { module: "tenants" }, subject: "Components" };
  const tenant = tenantSelection(hit.tenant);
  const query = hit.kind ? tenantComponentLocation(tenant, hit.kind) : tenantLocation(tenant);
  return {
    target: { module: "tenants", query },
    subject: `${attentionScope(hit.tenant)} · ${componentAttentionReason(hit)}`,
  };
}

export function defaultExpansion(data: TopologyData): Set<string> {
  const health = summarizeTopology(data);
  if (health.configAttention) {
    const hit = firstConfigAttentionHit(data);
    if (hit) {
      const base = tenantId(hit.tenant);
      return new Set([base, `${base}/agent:${hit.agent.agent}`]);
    }
  }
  if (health.componentAttention) {
    const hit = firstComponentAttentionHit(data);
    if (hit) {
      const base = tenantId(hit.tenant);
      return new Set([base, `${base}/components`]);
    }
  }
  return healthyDefaultExpansion(data);
}

export function firstComponentAttentionTarget(data: TopologyData): NavigationTarget {
  return firstComponentAttention(data).target;
}

export function componentAttentionItem(data: TopologyData, health: TopologyHealth): AttentionItem {
  const { target, subject } = firstComponentAttention(data);
  return {
    label: "Components",
    detail: attentionTargetDetail(subject, health.componentAttention),
    tone: health.componentErrors ? "error" : "warning",
    target,
  };
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
      summary.configTotal += agent.named_configs.count;
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
      for (const entry of agent.named_configs.attention) {
        if (entry.state === "incomplete" || entry.state === "invalid") summary.configAttention += 1;
        if (entry.state === "invalid") summary.configErrors += 1;
      }
    }
    if (tenant.components.error) {
      summary.componentAttention += 1;
      summary.componentErrors += 1;
    }
    summary.componentInstalled += tenant.components.installed;
    for (const entry of tenant.components.attention) {
      if (entry.error || ["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) {
        summary.componentAttention += 1;
      }
      if (entry.error) summary.componentErrors += 1;
    }
  }
  return summary;
}
export function attentionValue(value: number): string {
  return value === 0 ? "Healthy" : attentionCountLabel(value);
}

export type AttentionPanelKind = "items" | "pending" | "healthy";

/**
 * The healthy empty copy is a positive claim. It may appear only after both
 * Overview and topology have settled (data or error). Known items render as
 * soon as they exist, including while the other source is still loading.
 */
export function attentionPanelKind(input: {
  itemCount: number;
  overviewSettled: boolean;
  topologySettled: boolean;
}): AttentionPanelKind {
  if (input.itemCount > 0) return "items";
  if (!input.overviewSettled || !input.topologySettled) return "pending";
  return "healthy";
}
export function healthTone(attention?: number, errors?: number, loadError?: string | null): Tone {
  if (loadError || errors) return "error";
  if (attention) return "warning";
  return attention === 0 ? "good" : "neutral";
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
