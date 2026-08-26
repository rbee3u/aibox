import { useEffect, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { TenantApi } from "@/api/tenants";
import { messageOf } from "@/shared/lib/errors";

/** Loads the Tenant catalog that every Tenant-scoped module selects from. */
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
