import type { TenantRow } from "@/api/core";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelectionValue } from "@/domain/tenant";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";

/**
 * Projects Control API Tenant rows into Selection Menu options.
 *
 * This lives in `features/common` rather than `shared/` because it needs both a
 * wire type from `api/` and a UI type from `shared/ui`, and `shared/` may not
 * import `api/`. Configs, Sessions, and Tenants each built this list
 * separately, including three copies of the same locale sort.
 */

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

/** A Managed Tenant row that carries the name its option value needs. */
type NamedManagedTenant = TenantRow & { kind: "managed"; name: string };

/** Managed Tenants sorted by name, skipping rows with no usable name. */
export function managedTenants(tenants: readonly TenantRow[]): NamedManagedTenant[] {
  return tenants
    .filter((tenant): tenant is NamedManagedTenant =>
      Boolean(tenant.kind === "managed" && tenant.name),
    )
    .sort((left, right) => left.name.localeCompare(right.name));
}

/** The Host Tenant row, when the Control API reported one. */
export function hostTenant(tenants: readonly TenantRow[]): TenantRow | null {
  return tenants.find((tenant) => tenant.kind === "host") ?? null;
}

/**
 * Tenant options with the Host Tenant first, then Managed Tenants by name.
 *
 * The Host Tenant is omitted when absent rather than shown as unavailable,
 * matching the Console rule that a missing scope stays quiet.
 */
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
            icon: <HostTenantIcon size={14} aria-hidden="true" />,
          },
        ]
      : []),
    ...managedTenants(tenants).map((tenant) => ({
      value: `managed:${tenant.name}` as const,
      label: tenant.display_name,
      summaryLabel: tenant.display_name,
      icon: <ManagedTenantIcon size={14} aria-hidden="true" />,
    })),
  ];
}

/** The display name for a Tenant Selection, falling back to its raw name. */
export function tenantSelectionLabel(
  tenants: readonly TenantRow[],
  selection: { kind: "host" } | { kind: "managed"; name: string },
): string {
  if (selection.kind === "host") return "Host Tenant";
  return (
    tenants.find((row) => row.kind === "managed" && row.name === selection.name)?.display_name ??
    selection.name
  );
}

/** Coding Agent options in the Console's fixed presentation order. */
export function agentSelectionOptions(
  agents: readonly CodingAgentKind[],
): SelectionOption<CodingAgentKind>[] {
  return agents.map((value) => ({
    value,
    label: value === "codex" ? "Codex" : "Claude",
    icon: <BrandIcon brand={brandForAgent(value)} size={14} />,
  }));
}
