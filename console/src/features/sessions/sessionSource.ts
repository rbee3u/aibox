import type { CodingAgentKind } from "@/domain/codingAgent";
import type { SessionRow } from "@/api/sessions";
import {
  tenantSelectionFromKey,
  type TenantSelection,
  type TenantSelectionKey,
} from "@/domain/tenant";

/** A Session list combines several Tenant-and-Agent scopes, tracked per row. */
export interface SessionSource {
  key: string;
  tenant: TenantSelection;
  tenantKey: TenantSelectionKey;
  tenantLabel: string;
  agent: CodingAgentKind;
  agentLabel: string;
}

export interface SourcedSession extends SessionRow {
  key: string;
  source: SessionSource;
}

export interface AggregatedSessionData {
  sessions: SourcedSession[];
  warnings: string[];
  partial: boolean;
}

export const SESSION_AGENT_OPTIONS: readonly {
  value: CodingAgentKind;
  label: string;
}[] = [
  { value: "codex", label: "Codex" },
  { value: "claude", label: "Claude" },
];

export function agentLabel(agent: CodingAgentKind): string {
  return SESSION_AGENT_OPTIONS.find((option) => option.value === agent)?.label ?? agent;
}

export function tenantSelectionFromSessionKey(key: TenantSelectionKey): TenantSelection {
  return tenantSelectionFromKey(key);
}

export function sessionTenantLabel(key: TenantSelectionKey): string {
  return key === "host" ? "Host Tenant" : `Tenant ${key.slice(8)}`;
}

export function sessionListTenantLabel(key: TenantSelectionKey): string {
  return key === "host" ? "Host Tenant" : key.slice(8);
}

export function visibleSessionSource(source: SessionSource): string {
  return `${source.tenantLabel} ${source.agentLabel}`;
}

export function visibleSessionListSource(source: SessionSource): string {
  return `${sessionListTenantLabel(source.tenantKey)} ${source.agentLabel}`;
}

export function accessibleSessionSource(source: SessionSource): string {
  return `${source.tenantLabel} · ${source.agentLabel}`;
}

export function sessionSource(
  tenantKey: TenantSelectionKey,
  agent: CodingAgentKind,
): SessionSource {
  return {
    key: JSON.stringify([tenantKey, agent]),
    tenant: tenantSelectionFromSessionKey(tenantKey),
    tenantKey,
    tenantLabel: sessionTenantLabel(tenantKey),
    agent,
    agentLabel: agentLabel(agent),
  };
}

export function sourcedSession(source: SessionSource, row: SessionRow): SourcedSession {
  return {
    ...row,
    key: JSON.stringify([source.tenantKey, source.agent, row.id]),
    source,
  };
}

/** Newest first, then by Tenant, Coding Agent, and Session id for stability. */
export function compareSessions(left: SourcedSession, right: SourcedSession): number {
  return (
    right.start_ts.localeCompare(left.start_ts) ||
    left.source.tenantLabel.localeCompare(right.source.tenantLabel) ||
    left.source.agentLabel.localeCompare(right.source.agentLabel) ||
    left.id.localeCompare(right.id)
  );
}

export function focusTargetAfterSessionDelete(rows: SourcedSession[], key: string): string | null {
  const index = rows.findIndex((row) => row.key === key);
  if (index < 0) return null;
  return rows[index + 1]?.key ?? rows[index - 1]?.key ?? null;
}
