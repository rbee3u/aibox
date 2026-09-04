import type { CodingAgentKind } from "@/domain/codingAgent";
import {
  DNS_LABEL_PATTERN,
  parseTenantSelectionValue,
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelection,
  type TenantSelectionValue,
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
export type ConfigDeleteTarget = {
  names: string[];
};
export type ConfigApplyTarget = {
  name: string;
};
export type ConfigPendingAction = {
  run: () => void | Promise<void>;
};
export function configTenantSelectionValue(tenant: TenantSelection): TenantSelectionValue {
  return tenantSelectionValue(tenant);
}
export function tenantSelectionFromConfigValue(key: TenantSelectionValue): TenantSelection {
  return tenantSelectionFromValue(key);
}
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
  agent: CodingAgentKind;
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
    tenant: tenantSelectionFromConfigValue(tenantSelectionValue),
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
  agent: CodingAgentKind,
  selection: ConfigSelection | null,
  file?: string | null,
): URLSearchParams {
  const query = new URLSearchParams();
  query.set("tenant", configTenantSelectionValue(tenant));
  query.set("agent", agent);
  if (selection?.current) query.set("current", "1");
  else if (selection && isNamedCatalog(selection)) query.set("named", "1");
  else if (selection) query.set("config", selection.config);
  if (selection && file && !isNamedCatalog(selection)) query.set("file", file);
  return query;
}
