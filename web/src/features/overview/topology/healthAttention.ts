import { tenantSelection, tenantSelectionValue } from "@/domain/tenant";
import { configLocation } from "@/features/common/routes/configs";
import { sessionLocation } from "@/features/common/routes/sessions";
import { tenantComponentLocation } from "@/features/common/routes/tenants";
import type { Operation } from "@/api/operations";
import type { OverviewData, TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import type { ComponentKind } from "@/api/tenants";
import { orderTenants } from "@/features/overview/tenantStatus";
import type {
  AttentionItem,
  AttentionPanelKind,
  NavigationTarget,
} from "@/features/overview/viewTypes";
import { agentLabel, capitalize } from "@/shared/lib/format";

function configAttentionTarget(
  tenant: TopologyTenant,
  agent: TopologyAgent,
  config?: string,
): NavigationTarget {
  const query = configLocation(
    tenantSelection(tenant),
    agent.agent,
    config ? { current: false, config } : { current: true },
  );
  return { module: "configs", query };
}

function attentionScope(tenant: TopologyTenant, agent?: TopologyAgent): string {
  return agent ? `${tenant.display_name} · ${agentLabel(agent.agent)}` : tenant.display_name;
}

function driftReason(drift: string): string {
  if (drift === "dirty") return "Current Config differs";
  if (drift === "source-missing") return "Current Config source is missing";
  return "Current Config comparison failed";
}

/**
 * Keep each condition separately actionable, including multiple failures per Tenant.
 */
export function configAttentions(data: TopologyData): AttentionItem[] {
  const items: AttentionItem[] = [];
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      const scope = attentionScope(tenant, agent);
      const id = `${tenantSelectionValue(tenant)}/${agent.agent}`;
      if (agent.current_config.error)
        items.push({
          key: `config-current-error:${id}`,
          label: "Configs",
          detail: `${scope} · Current Config inspection failed`,
          tone: "error",
          target: configAttentionTarget(tenant, agent),
        });
      if (["dirty", "source-missing", "comparison-error"].includes(agent.application.drift))
        items.push({
          key: `config-drift:${id}`,
          label: "Configs",
          detail: `${scope} · ${driftReason(agent.application.drift)}`,
          tone: agent.application.drift === "comparison-error" ? "error" : "warning",
          target: configAttentionTarget(tenant, agent),
        });
      if (agent.named_configs.error)
        items.push({
          key: `config-catalog:${id}`,
          label: "Configs",
          detail: `${scope} · Named Configs inspection failed`,
          tone: "error",
          target: {
            module: "configs",
            query: configLocation(tenantSelection(tenant), agent.agent, {
              current: false,
              namedCatalog: true,
            }),
          },
        });
      for (const entry of agent.named_configs.attention) {
        if (entry.state !== "incomplete" && entry.state !== "invalid") continue;
        items.push({
          key: `config-named:${id}/${entry.name}`,
          label: "Configs",
          detail: `${scope} · ${entry.name} is ${entry.state === "invalid" ? "invalid" : "incomplete"}`,
          tone: entry.state === "invalid" ? "error" : "warning",
          target: configAttentionTarget(tenant, agent, entry.name),
        });
      }
    }
  }
  return items;
}

function componentAttentionReason(input: {
  kind: ComponentKind | null;
  status?: string | null;
  error?: string | null;
}): string {
  if (!input.kind) return "Components inspection failed";
  const label = input.kind.split("-").map(capitalize).join(" ");
  if (input.error) return `${label} inspection failed`;
  if (input.status === "modified") return `${label} differs from the AIBox definition`;
  if (input.status === "incomplete") return `${label} is incomplete`;
  if (input.status === "unmanaged") return `${label} is unmanaged`;
  return label;
}

/** Every Component condition the topology reports, one item each. */
export function componentAttentions(data: TopologyData): AttentionItem[] {
  const items: AttentionItem[] = [];
  for (const tenant of orderTenants(data.tenants)) {
    const scope = attentionScope(tenant);
    const selection = tenantSelection(tenant);
    const id = tenantSelectionValue(tenant);
    if (tenant.components.error)
      items.push({
        key: `component-catalog:${id}`,
        label: "Components",
        detail: `${scope} · ${componentAttentionReason({ kind: null })}`,
        tone: "error",
        target: { module: "tenants", query: tenantComponentLocation(selection) },
      });
    for (const entry of tenant.components.attention) {
      if (!entry.error && !["modified", "incomplete", "unmanaged"].includes(entry.status ?? ""))
        continue;
      items.push({
        key: `component:${id}/${entry.kind}`,
        label: "Components",
        detail: `${scope} · ${componentAttentionReason(entry)}`,
        tone: entry.error ? "error" : "warning",
        target: { module: "tenants", query: tenantComponentLocation(selection, entry.kind) },
      });
    }
  }
  return items;
}

/**
 * Only discovery failures need attention; an empty Session catalog is valid.
 */
export function sessionAttentions(data: TopologyData): AttentionItem[] {
  const items: AttentionItem[] = [];
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      if (!agent.sessions.error) continue;
      const query = sessionLocation(tenantSelection(tenant), agent.agent);
      items.push({
        key: `sessions:${tenantSelectionValue(tenant)}/${agent.agent}`,
        label: "Sessions",
        detail: `${attentionScope(tenant, agent)} · Session inspection failed`,
        tone: "error",
        target: { module: "sessions", query },
      });
    }
  }
  return items;
}

/** Config, Session, and Component conditions together, in Tenant order. */
export function topologyAttentions(data: TopologyData): AttentionItem[] {
  return [...configAttentions(data), ...sessionAttentions(data), ...componentAttentions(data)];
}

/**
 * Sort errors before warnings, preserving collection order within each severity.
 */
export function bySeverity(items: AttentionItem[]): AttentionItem[] {
  return [...items].sort(
    (left, right) => (left.tone === "error" ? 0 : 1) - (right.tone === "error" ? 0 : 1),
  );
}

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
export function buildDisabledReason(
  data: OverviewData | null,
  operation: Operation | null,
): string {
  if (operation?.state === "running") return `Unavailable while ${operation.kind} is running`;
  if (data?.docker.status === "unavailable") return data.docker.error ?? "Docker is unavailable";
  return "Status is still loading";
}
