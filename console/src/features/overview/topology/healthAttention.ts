import { formatBinaryByteSize as formatBytes } from "@/shared/lib/encoding";
import type { Operation } from "@/api/operations";
import type { OverviewData, TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import { orderTenants, type NavigationTarget } from "@/features/overview/topology/coreTree";
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
    const hasAttention = tenant.components.entries.some(
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
