import type { Operation } from "@/api/operations";
import type { OverviewData, TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import {
  attentionCountLabel,
  namedCatalogLocation,
  orderTenants,
  tenantSelection,
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
export function firstConfigAttentionTarget(data: TopologyData): NavigationTarget {
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      if (
        agent.current_config.error ||
        ["dirty", "source-missing", "comparison-error"].includes(agent.application.drift)
      ) {
        return configAttentionTarget(tenant, agent);
      }
      const entry = agent.named_configs.attention.find(
        (candidate) => candidate.state === "incomplete" || candidate.state === "invalid",
      );
      if (entry) return configAttentionTarget(tenant, agent, entry.name);
      if (agent.named_configs.error) {
        return {
          module: "configs",
          query: namedCatalogLocation(tenantSelection(tenant), agent.agent),
        };
      }
    }
  }
  return { module: "configs" };
}
export function firstComponentAttentionTarget(data: TopologyData): NavigationTarget {
  for (const tenant of orderTenants(data.tenants)) {
    const query = new URLSearchParams();
    query.set("tenant", attentionTenant(tenant));
    const hasAttention = tenant.components.attention.some(
      (candidate) =>
        candidate.error || ["modified", "incomplete", "unmanaged"].includes(candidate.status ?? ""),
    );
    if (hasAttention || tenant.components.error) return { module: "tenants", query };
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
