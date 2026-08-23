import { useEffect, useState } from "react";
import type { TenantApi, TenantRow } from "./controlApi";
import type { ModuleId } from "./consoleIcons";
export {
  DNS_LABEL_PATTERN,
  parseTenantSelectionKey,
  type TenantSelectionKey,
} from "./tenantSelection";

export type PageLocationChange = (
  module: ModuleId,
  query: URLSearchParams,
  replace?: boolean,
) => void;

export function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

export function currentPageSearch(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}

export function changePageLocation(
  module: ModuleId,
  query: URLSearchParams,
  onLocationChange?: PageLocationChange,
  replace = false,
) {
  onLocationChange?.(module, query, replace);
}

export function useTenants(api: Pick<TenantApi, "listTenants">) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    let disposed = false;
    // This hook synchronizes its local snapshot with the external Tenant catalog.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoading(true);
    void api
      .listTenants()
      .then((rows) => {
        if (disposed) return;
        setTenants(rows);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!disposed) setError(messageOf(cause));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [api, generation]);

  return {
    tenants,
    loading,
    error,
    retry: () => setGeneration((value) => value + 1),
  };
}
