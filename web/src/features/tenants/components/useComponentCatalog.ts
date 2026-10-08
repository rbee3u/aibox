import { useCallback, useEffect, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { ComponentRow, TenantApi } from "@/api/tenants";
import { tenantSelection, tenantSelectionValue } from "@/domain/tenant";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

type ComponentCatalogApi = Pick<TenantApi, "listComponents">;

/** Owns Component inspection state for one Tenant. */
export function useComponentCatalog(
  api: ComponentCatalogApi,
  onError: (message: string | null) => void,
) {
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [loadedTenantKey, setLoadedTenantKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const catalogRequest = useRef(new LatestRequest());

  const load = useCallback(
    async (target: TenantRow | null, showLoading = false): Promise<ComponentRow[] | null> => {
      if (!target) {
        catalogRequest.current.cancel();
        setComponents([]);
        setLoadedTenantKey(null);
        setLoading(false);
        return [];
      }
      if (showLoading) setLoading(true);
      return catalogRequest.current.run(
        (signal) => api.listComponents(tenantSelection(target), signal),
        {
          loaded: (rows) => {
            setComponents(rows);
            setLoadedTenantKey(tenantSelectionValue(target));
            onError(null);
          },
          failed: (cause) => onError(messageOf(cause)),
          settled: () => setLoading(false),
        },
      );
    },
    [api, onError],
  );

  useEffect(() => {
    const catalogOwner = catalogRequest.current;
    return () => catalogOwner.cancel();
  }, []);

  return {
    components,
    load,
    loading,
    loadedTenantKey,
  };
}
