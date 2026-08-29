import type { CodingAgentKind } from "@/domain/codingAgent";
import {
  DNS_LABEL_PATTERN,
  parseTenantSelectionKey,
  tenantSelectionFromKey,
  tenantSelectionValue,
  type TenantSelection,
  type TenantSelectionKey,
} from "@/domain/tenant";

export type ConfigSelection =
  | {
      current: true;
      config?: never;
    }
  | {
      current: false;
      config: string;
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
export function configTenantKey(tenant: TenantSelection): TenantSelectionKey {
  return tenantSelectionValue(tenant);
}
export function tenantSelectionFromConfigKey(key: TenantSelectionKey): TenantSelection {
  return tenantSelectionFromKey(key);
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
  const tenantKey = parseTenantSelectionKey(query.get("tenant")) ?? "managed:default";
  const agent = query.get("agent") === "claude" ? "claude" : "codex";
  const config = query.get("config");
  const current = query.get("current") === "1";
  const detailOpen = current || (config !== null && DNS_LABEL_PATTERN.test(config));
  return {
    tenant: tenantSelectionFromConfigKey(tenantKey),
    agent,
    selection:
      !current && config && DNS_LABEL_PATTERN.test(config)
        ? { current: false, config }
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
  query.set("tenant", configTenantKey(tenant));
  query.set("agent", agent);
  if (selection?.current) query.set("current", "1");
  else if (selection) query.set("config", selection.config);
  if (selection && file) query.set("file", file);
  return query;
}
