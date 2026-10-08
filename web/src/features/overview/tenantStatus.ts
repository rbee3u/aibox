import { configLocation } from "@/features/common/routes/configs";
import { sessionLocation } from "@/features/common/routes/sessions";
import { tenantComponentLocation } from "@/features/common/routes/tenants";
import type {
  TopologyAgent,
  TopologyComponents,
  TopologyData,
  TopologyTenant,
} from "@/api/overview";
import type { ComponentRow } from "@/api/tenants";
import { tenantSelection, tenantSelectionValue, type TenantSelection } from "@/domain/tenant";
import { driftCatalogLabel } from "@/shared/lib/format";
import type {
  AgentStatus,
  ResourceStatus,
  TenantStatusRow,
  Tone,
} from "@/features/overview/viewTypes";

/** Project the reported topology directly into Tenant status rows. */
export function tenantStatusRows(data: TopologyData): TenantStatusRow[] {
  return orderTenants(data.tenants).map((row) => {
    const tenant = tenantSelection(row);
    const agents: TenantStatusRow["agents"] = {};
    for (const kind of ["codex", "claude"] as const) {
      const agent = row.agents.find((entry) => entry.agent === kind);
      if (agent) agents[kind] = agentStatus(tenant, agent);
    }
    return {
      id: tenantSelectionValue(row),
      kind: row.kind,
      label: row.display_name,
      detail: row.exists ? undefined : "Home unavailable",
      tone: row.exists ? "neutral" : "warning",
      target: { module: "tenants", query: tenantComponentLocation(tenant) },
      components: componentStatus(tenant, row.components),
      agents,
    };
  });
}

function agentStatus(tenant: TenantSelection, agent: TopologyAgent): AgentStatus {
  const currentQuery = configLocation(tenant, agent.agent, { current: true });
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
  const sessionQuery = sessionLocation(tenant, agent.agent);
  return {
    applicationLabel: agent.application.last_application
      ? `Last applied: ${agent.application.last_application.applied}`
      : undefined,
    current: {
      label: "Current Config",
      detail: agent.current_config.error
        ? "Inspection failed"
        : currentTone === "neutral"
          ? undefined
          : driftCatalogLabel(agent.application.drift),
      tone: currentTone,
      target: { module: "configs", query: currentQuery },
    },
    configs: {
      label: "Named Configs",
      detail: agent.named_configs.error
        ? "Catalog inspection failed"
        : `${agent.named_configs.count} Named Configs${namedAttention.length ? ` · ${attentionCountLabel(namedAttention.length)}` : ""}`,
      tone: namedTone,
      target: {
        module: "configs",
        query: configLocation(tenant, agent.agent, { current: false, namedCatalog: true }),
      },
    },
    sessions: {
      label: "Sessions",
      detail: agent.sessions.error
        ? "Session inspection failed"
        : `${agent.sessions.count} Sessions`,
      tone: agent.sessions.error ? "error" : "neutral",
      target: { module: "sessions", query: sessionQuery },
    },
  };
}

function componentStatus(tenant: TenantSelection, summary: TopologyComponents): ResourceStatus {
  const attention = summary.attention.map(componentTone).filter((tone) => tone !== "neutral");
  return {
    label: "Components",
    detail: summary.error
      ? "Catalog inspection failed"
      : `${summary.installed}/${summary.total} installed${attention.length ? ` · ${attentionCountLabel(attention.length)}` : ""}`,
    tone:
      summary.error || attention.includes("error")
        ? "error"
        : attention.length > 0
          ? "warning"
          : "neutral",
    target: { module: "tenants", query: tenantComponentLocation(tenant) },
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

function currentConfigTone(agent: TopologyAgent): Tone {
  if (agent.current_config.error || agent.application.drift === "comparison-error") return "error";
  if (agent.application.drift === "dirty" || agent.application.drift === "source-missing")
    return "warning";
  return "neutral";
}

function componentTone(entry: ComponentRow): "neutral" | "warning" | "error" {
  if (entry.error) return "error";
  if (["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) return "warning";
  return "neutral";
}

function attentionCountLabel(count: number): string {
  return `${count} ${count === 1 ? "needs" : "need"} attention`;
}
