import type { AgentKind } from "@/domain/agent";
import {
  DNS_LABEL_PATTERN,
  parseTenantSelectionValue,
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelection,
} from "@/domain/tenant";

export type ConfigSelection =
  | {
      current: true;
      config?: never;
      namedCatalog?: never;
    }
  | {
      current: false;
      config: string;
      namedCatalog?: never;
    }
  | {
      current: false;
      namedCatalog: true;
      config?: never;
    };
export function isNamedCatalog(
  selection: ConfigSelection,
): selection is { current: false; namedCatalog: true } {
  return selection.namedCatalog === true;
}
export function namedConfigName(selection: ConfigSelection): string | null {
  return selection.config ?? null;
}
export interface ConfigRouteState {
  tenant: TenantSelection;
  agent: AgentKind;
  selection: ConfigSelection;
  file: string | null;
  detailOpen: boolean;
}
export function readConfigRoute(search: string): ConfigRouteState {
  const query = new URLSearchParams(search);
  const tenantSelectionValue = parseTenantSelectionValue(query.get("tenant")) ?? "managed:default";
  const agent = query.get("agent") === "claude" ? "claude" : "codex";
  const config = query.get("config");
  const current = query.get("current") === "1";
  const namedCatalog = query.get("named") === "1";
  const namedConfig = !current && config && DNS_LABEL_PATTERN.test(config) ? config : null;
  const detailOpen = current || namedConfig !== null;
  return {
    tenant: tenantSelectionFromValue(tenantSelectionValue),
    agent,
    selection: namedConfig
      ? { current: false, config: namedConfig }
      : namedCatalog && !current
        ? { current: false, namedCatalog: true }
        : { current: true },
    file: detailOpen ? query.get("file") : null,
    detailOpen,
  };
}
export function configLocation(
  tenant: TenantSelection,
  agent: AgentKind,
  selection: ConfigSelection | null,
  file?: string | null,
): URLSearchParams {
  const query = new URLSearchParams();
  query.set("tenant", tenantSelectionValue(tenant));
  query.set("agent", agent);
  if (selection?.current) query.set("current", "1");
  else if (selection && isNamedCatalog(selection)) query.set("named", "1");
  else if (selection) query.set("config", selection.config);
  if (selection && file && !isNamedCatalog(selection)) query.set("file", file);
  return query;
}
