import type { AgentKind } from "@/domain/agent";
import {
  parseTenantSelectionValue,
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelection,
  type TenantSelectionValue,
} from "@/domain/tenant";
import { readEnum } from "@/shared/lib/queryParams";

export type SessionTab = "conversation" | "details";

export interface SessionRouteState {
  tenant: TenantSelection;
  agent: AgentKind;
  sessionId: string | null;
  tab: SessionTab;
}

const SESSION_TABS: readonly SessionTab[] = ["conversation", "details"];

export function sessionTenantSelectionValue(tenant: TenantSelection): TenantSelectionValue {
  return tenantSelectionValue(tenant);
}

export function tenantSelectionFromSessionValue(key: TenantSelectionValue): TenantSelection {
  return tenantSelectionFromValue(key);
}

/**
 * Sessions is scoped by a single `tenant` and `agent`, with an optional `session`
 * selected. Missing or unparsable values fall back to the Default Managed
 * Tenant and Codex.
 */
export function readSessionRoute(search: string): SessionRouteState {
  const query = new URLSearchParams(search);
  const tenantSelectionValue = parseTenantSelectionValue(query.get("tenant")) ?? "managed:default";
  const rawAgent = query.get("agent");
  const agent: AgentKind = rawAgent === "claude" ? "claude" : "codex";
  const sessionId = query.get("session");
  return {
    tenant: tenantSelectionFromSessionValue(tenantSelectionValue),
    agent,
    sessionId: sessionId && sessionId.length > 0 ? sessionId : null,
    tab: readEnum(query, "tab", SESSION_TABS, "conversation"),
  };
}

export function sessionLocation(
  tenant: TenantSelection,
  agent: AgentKind,
  sessionId?: string | null,
  tab: SessionTab = "conversation",
): URLSearchParams {
  const query = new URLSearchParams();
  query.set("tenant", sessionTenantSelectionValue(tenant));
  query.set("agent", agent);
  if (sessionId) {
    query.set("session", sessionId);
    if (tab === "details") query.set("tab", tab);
  }
  return query;
}
