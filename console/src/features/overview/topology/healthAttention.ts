import type { Operation } from "@/api/operations";
import type { OverviewData, TopologyAgent, TopologyData, TopologyTenant } from "@/api/overview";
import type { ComponentKind } from "@/api/tenants";
import {
  componentLabel,
  namedCatalogLocation,
  orderTenants,
  tenantComponentLocation,
  tenantLocation,
  tenantSelection,
  type AttentionItem,
  type NavigationTarget,
} from "@/features/overview/resourceTree";
import type { AttentionPanelKind } from "@/features/overview/viewTypes";

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

function agentTitle(agent: TopologyAgent): string {
  return agent.agent === "codex" ? "Codex" : "Claude";
}

function attentionScope(tenant: TopologyTenant, agent?: TopologyAgent): string {
  return agent ? `${tenant.display_name} · ${agentTitle(agent)}` : tenant.display_name;
}

function driftReason(drift: string): string {
  if (drift === "dirty") return "Current Config differs";
  if (drift === "source-missing") return "Current Config source is missing";
  return "Current Config comparison failed";
}

/**
 * Every Config condition the topology reports, one item each.
 *
 * The panel used to show the first hit per category and fold the rest into a
 * `+N more` tail inside the detail sentence, which named one Tenant and left
 * the reader to open a module to discover the others. The conditions are the
 * same ones the health summary counted, so enumerating them keeps the panel's
 * row count equal to the number of real problems.
 */
export function configAttentions(data: TopologyData): AttentionItem[] {
  const items: AttentionItem[] = [];
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      const scope = attentionScope(tenant, agent);
      const id = `${attentionTenant(tenant)}/${agent.agent}`;
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
            query: namedCatalogLocation(tenantSelection(tenant), agent.agent),
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
  const label = componentLabel(input.kind);
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
    const id = attentionTenant(tenant);
    if (tenant.components.error)
      items.push({
        key: `component-catalog:${id}`,
        label: "Components",
        detail: `${scope} · ${componentAttentionReason({ kind: null })}`,
        tone: "error",
        target: { module: "tenants", query: tenantLocation(selection) },
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
 * Every Session condition the topology reports, one item each.
 *
 * Discovery only counts Transcripts, so the single condition is a Home that
 * could not be walked. A count of zero is a fact, not a problem, and stays out
 * of the panel.
 */
export function sessionAttentions(data: TopologyData): AttentionItem[] {
  const items: AttentionItem[] = [];
  for (const tenant of orderTenants(data.tenants)) {
    for (const agent of tenant.agents) {
      if (!agent.sessions.error) continue;
      const query = tenantLocation(tenantSelection(tenant));
      query.set("agent", agent.agent);
      items.push({
        key: `sessions:${attentionTenant(tenant)}/${agent.agent}`,
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
 * Errors first, then warnings, each keeping the order it was collected in.
 *
 * Collection order is the order the sources are read — Service, then Docker,
 * then the Runtime Image, then the topology — which is an implementation
 * detail, not a priority. `Array.prototype.sort` is stable, so ranking by tone
 * alone leaves that order intact inside each tone.
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
