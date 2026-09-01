import { useCallback, useEffect, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { ComponentRow, TenantApi } from "@/api/tenants";
import { tenantSelection } from "@/features/tenants/componentCatalog";
import { tenantSelectionValueOf } from "@/features/tenants/route";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

type ComponentCatalogApi = Pick<TenantApi, "listComponents">;

/** Owns Component inspection state for one Tenant. */
export function useComponentCatalog(
  api: ComponentCatalogApi,
  onError: (message: string | null) => void,
) {
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [tenantSelectionValue, setTenantSelectionValue] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const catalogRequest = useRef(new LatestRequest());
  const preserveError = useRef(false);

  const load = useCallback(
    async (target: TenantRow | null, showLoading = false): Promise<ComponentRow[] | null> => {
      const request = catalogRequest.current.begin();
      if (!target) {
        request.release();
        setComponents([]);
        setTenantSelectionValue(null);
        setLoading(false);
        return [];
      }
      if (showLoading) setLoading(true);
      try {
        const rows = await api.listComponents(tenantSelection(target), request.signal);
        if (request.signal.aborted || !request.isCurrent()) return null;
        setComponents(rows);
        setTenantSelectionValue(tenantSelectionValueOf(target));
        if (preserveError.current) preserveError.current = false;
        else onError(null);
        return rows;
      } catch (cause) {
        if (request.signal.aborted || !request.isCurrent()) return null;
        if (preserveError.current) preserveError.current = false;
        else onError(messageOf(cause));
        return null;
      } finally {
        if (request.isCurrent()) {
          request.release();
          setLoading(false);
        }
      }
    },
    [api, onError],
  );

  const preserveNextError = useCallback(() => {
    preserveError.current = true;
  }, []);

  useEffect(() => {
    const catalogOwner = catalogRequest.current;
    return () => catalogOwner.cancel();
  }, []);

  return {
    components,
    load,
    loading,
    preserveNextError,
    tenantSelectionValue,
  };
}
