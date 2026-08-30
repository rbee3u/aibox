import type { CodingAgentKind } from "@/domain/codingAgent";
import { parseTenantSelectionValue, type TenantSelectionValue } from "@/domain/tenant";
import { SESSION_AGENT_OPTIONS } from "@/features/sessions/sessionSource";
import { readEnum } from "@/shared/lib/queryParams";

export type SessionTab = "conversation" | "details";

export interface SessionRouteSelection {
  tenantSelectionValue: TenantSelectionValue;
  agent: CodingAgentKind;
  id: string;
}

export interface SessionRouteState {
  tenants: Set<TenantSelectionValue>;
  agents: Set<CodingAgentKind>;
  selection: SessionRouteSelection | null;
  tab: SessionTab;
}

const SESSION_TABS: readonly SessionTab[] = ["conversation", "details"];

function isAgent(value: string | null): value is CodingAgentKind {
  return value === "codex" || value === "claude";
}

/**
 * Sessions is scoped by repeated `tenant` and `agent` values, with the selected
 * Session named separately. An empty selection falls back to the Default Managed
 * Tenant and Codex so the module always has something to list.
 */
export function readSessionRoute(search: string): SessionRouteState {
  const query = new URLSearchParams(search);
  const tenants = new Set(
    query
      .getAll("tenant")
      .map(parseTenantSelectionValue)
      .filter((value): value is TenantSelectionValue => value !== null),
  );
  const agents = new Set(query.getAll("agent").filter(isAgent));
  if (tenants.size === 0) tenants.add("managed:default");
  if (agents.size === 0) agents.add("codex");
  const selectedTenant = parseTenantSelectionValue(query.get("session_tenant"));
  const selectedAgent = query.get("session_agent");
  const id = query.get("session");
  const selection: SessionRouteSelection | null =
    selectedTenant && isAgent(selectedAgent) && id
      ? { tenantSelectionValue: selectedTenant, agent: selectedAgent, id }
      : null;
  return {
    tenants,
    agents,
    selection,
    tab: readEnum(query, "tab", SESSION_TABS, "conversation"),
  };
}

export function sessionLocation(
  tenants: ReadonlySet<TenantSelectionValue>,
  agents: ReadonlySet<CodingAgentKind>,
  selection?: SessionRouteSelection | null,
  tab: SessionTab = "conversation",
): URLSearchParams {
  const query = new URLSearchParams();
  for (const tenant of [...tenants].sort()) query.append("tenant", tenant);
  for (const agent of SESSION_AGENT_OPTIONS.map((option) => option.value)) {
    if (agents.has(agent)) query.append("agent", agent);
  }
  if (selection) {
    query.set("session_tenant", selection.tenantSelectionValue);
    query.set("session_agent", selection.agent);
    query.set("session", selection.id);
    if (tab === "details") query.set("tab", tab);
  }
  return query;
}
