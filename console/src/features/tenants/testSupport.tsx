import type { ComponentProps } from "react";
import { vi } from "vitest";
import type { TenantRow } from "@/api/core";
import type { ComponentLatestSnapshot, ComponentRow, TenantApi } from "@/api/tenants";
import { TenantPage as TenantPageView } from "@/features/tenants/TenantPage";
import { useTestLocation } from "@/test/useTestLocation";

/**
 * Tenants-specific rows: `default` sits under the Host Home so the page
 * abbreviates it to `~/...`, while `work` is outside and must stay absolute.
 * The shared TENANT_ROWS fixture cannot express that, so these stay local.
 */
export const tenantRows = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/home/test/.aibox/tenants/default",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/var/lib/aibox/tenants/work",
    exists: true,
  },
] satisfies TenantRow[];

export function TenantPage(
  props: Omit<ComponentProps<typeof TenantPageView>, "search" | "onLocationChange"> & {
    api: TenantApi;
    search?: string;
    onLocationChange?: ComponentProps<typeof TenantPageView>["onLocationChange"];
  },
) {
  const { api, search, onLocationChange: notify, ...pageProps } = props;
  const location = useTestLocation(search, notify);
  return (
    <TenantPageView
      api={api}
      search={location.currentSearch}
      onLocationChange={location.onLocationChange}
      {...pageProps}
    />
  );
}

export function tenantApi(
  options: {
    rows?: TenantRow[];
    components?: ComponentRow[];
    latest?: ComponentLatestSnapshot | null;
    listTenants?: TenantApi["listTenants"];
    listComponents?: TenantApi["listComponents"];
    latestComponents?: TenantApi["latestComponents"];
    checkLatestComponents?: TenantApi["checkLatestComponents"];
    createTenant?: TenantApi["createTenant"];
    deleteTenants?: TenantApi["deleteTenants"];
    mutateComponent?: TenantApi["mutateComponent"];
  } = {},
) {
  const unconfigured = (operation: string) =>
    Promise.reject(new Error(`${operation} was not configured for this test`));
  const rows = options.rows ?? tenantRows;
  const components = options.components ?? [];
  const latest = options.latest ?? null;
  const listTenants = vi.fn<TenantApi["listTenants"]>(
    options.listTenants ?? (() => Promise.resolve(rows)),
  );
  const listComponents = vi.fn<TenantApi["listComponents"]>(
    options.listComponents ?? (() => Promise.resolve(components)),
  );
  const latestComponents = vi.fn<TenantApi["latestComponents"]>(
    options.latestComponents ?? (() => Promise.resolve(latest)),
  );
  const checkLatestComponents = vi.fn<TenantApi["checkLatestComponents"]>(
    options.checkLatestComponents ??
      (() =>
        latest
          ? Promise.resolve(latest)
          : Promise.reject(new Error("No Latest Release snapshot configured"))),
  );
  const createTenant = vi.fn<TenantApi["createTenant"]>(
    options.createTenant ?? (() => unconfigured("createTenant")),
  );
  const deleteTenants = vi.fn<TenantApi["deleteTenants"]>(
    options.deleteTenants ?? (() => unconfigured("deleteTenants")),
  );
  const mutateComponent = vi.fn<TenantApi["mutateComponent"]>(
    options.mutateComponent ?? (() => unconfigured("mutateComponent")),
  );
  const api = {
    listTenants,
    listComponents,
    latestComponents,
    checkLatestComponents,
    createTenant,
    deleteTenants,
    mutateComponent,
  } satisfies TenantApi;
  return {
    api,
    listTenants,
    listComponents,
    latestComponents,
    checkLatestComponents,
    createTenant,
    deleteTenants,
    mutateComponent,
  };
}
