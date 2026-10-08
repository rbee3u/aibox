import { useCallback, useEffect, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { TenantApi } from "@/api/tenants";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

/**
 * Keep loaded rows mounted during reload so mutation and Refresh preserve focus.
 */
export function useTenantCatalog(api: Pick<TenantApi, "listTenants">) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestOwner = useRef(new LatestRequest());
  const loadedOnce = useRef(false);

  const load = useCallback(async (): Promise<TenantRow[] | null> => {
    if (!loadedOnce.current) setLoading(true);
    return requestOwner.current.run((signal) => api.listTenants(signal), {
      loaded: (rows) => {
        loadedOnce.current = true;
        setTenants(rows);
        setError(null);
      },
      failed: (cause) => setError(messageOf(cause)),
      settled: () => setLoading(false),
    });
  }, [api]);

  useEffect(() => {
    void load();
    const owner = requestOwner.current;
    return () => owner.cancel();
  }, [load]);

  return { tenants, loading, error, load };
}
