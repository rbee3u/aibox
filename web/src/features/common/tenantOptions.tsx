import type { TenantRow } from "@/api/core";
import { AGENTS, type AgentKind } from "@/domain/agent";
import {
  tenantSelectionValue,
  type TenantSelection,
  type TenantSelectionValue,
} from "@/domain/tenant";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";
import { iconSize } from "@/shared/icons/iconSizes";
import { agentLabel } from "@/shared/lib/format";

// This combines API and UI types, so it belongs in features/common, not shared.

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

type NamedManagedTenant = TenantRow & { kind: "managed"; name: string };

export function managedTenants(tenants: readonly TenantRow[]): NamedManagedTenant[] {
  return tenants
    .filter((tenant): tenant is NamedManagedTenant =>
      Boolean(tenant.kind === "managed" && tenant.name),
    )
    .sort((left, right) => left.name.localeCompare(right.name));
}

export function hostTenant(tenants: readonly TenantRow[]): TenantRow | null {
  return tenants.find((tenant) => tenant.kind === "host") ?? null;
}

export function tenantSelectionOptions(
  tenants: readonly TenantRow[],
): SelectionOption<TenantSelectionValue>[] {
  const host = hostTenant(tenants);
  return [
    ...(host
      ? [
          {
            value: "host" as const,
            label: "Host Tenant",
            summaryLabel: "Host",
            icon: <HostTenantIcon size={iconSize.xs} aria-hidden="true" />,
          },
        ]
      : []),
    ...managedTenants(tenants).map((tenant) => ({
      value: tenantSelectionValue(tenant),
      label: tenant.display_name,
      summaryLabel: tenant.display_name,
      icon: <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />,
    })),
  ];
}

export function tenantSelectionLabel(
  tenants: readonly TenantRow[],
  selection: TenantSelection,
): string {
  if (selection.kind === "host") return "Host Tenant";
  return (
    tenants.find((row) => row.kind === "managed" && row.name === selection.name)?.display_name ??
    selection.name
  );
}

export function agentSelectionOptions(): SelectionOption<AgentKind>[] {
  return AGENTS.map((value) => ({
    value,
    label: agentLabel(value),
    icon: <BrandIcon brand={brandForAgent(value)} size={iconSize.xs} />,
  }));
}
