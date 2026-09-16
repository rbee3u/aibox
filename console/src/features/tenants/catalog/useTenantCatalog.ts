import { useCallback, useEffect, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { TenantApi } from "@/api/tenants";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

/**
 * Owns the asynchronous Tenant catalog snapshot used by the Tenants page.
 *
 * `loading` is true only until the first snapshot arrives. A reload keeps the
 * rows it already has on screen, so a create, delete, or Refresh neither
 * blanks the list nor unmounts the row or control that focus should return to.
 */
export function useTenantCatalog(api: Pick<TenantApi, "listTenants">) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestOwner = useRef(new LatestRequest());
  const loadedOnce = useRef(false);

  const load = useCallback(async (): Promise<TenantRow[] | null> => {
    if (!loadedOnce.current) setLoading(true);
    const request = requestOwner.current.begin();
    try {
      const rows = await api.listTenants(request.signal);
      if (request.signal.aborted || !request.isCurrent()) return null;
      loadedOnce.current = true;
      setTenants(rows);
      setError(null);
      return rows;
    } catch (cause) {
      if (!request.signal.aborted) setError(messageOf(cause));
      return null;
    } finally {
      if (request.isCurrent()) {
        request.release();
        setLoading(false);
      }
    }
  }, [api]);

  useEffect(() => {
    // The catalog is an external resource; the page consumes its immutable snapshot.
    void load();
    const owner = requestOwner.current;
    return () => owner.cancel();
  }, [load]);

  return { tenants, loading, error, load };
}
