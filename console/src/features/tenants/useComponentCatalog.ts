import { useCallback, useEffect, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { ComponentLatestSnapshot, ComponentRow, TenantApi } from "@/api/tenants";
import { tenantSelection } from "@/features/tenants/componentCatalog";
import { tenantKeyOf } from "@/features/tenants/route";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

type ComponentCatalogApi = Pick<
  TenantApi,
  "listComponents" | "latestComponents" | "checkLatestComponents"
>;

/** Owns Component inspection and Latest Release observation for one Tenant. */
export function useComponentCatalog(
  api: ComponentCatalogApi,
  onError: (message: string | null) => void,
) {
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [tenantKey, setTenantKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [latestSnapshot, setLatestSnapshot] = useState<ComponentLatestSnapshot | null>(null);
  const [checkingLatest, setCheckingLatest] = useState(false);
  const catalogRequest = useRef(new LatestRequest());
  const latestRequest = useRef(new LatestRequest());
  const preserveError = useRef(false);

  const load = useCallback(
    async (target: TenantRow | null, showLoading = false): Promise<ComponentRow[] | null> => {
      const request = catalogRequest.current.begin();
      if (!target) {
        request.release();
        setComponents([]);
        setTenantKey(null);
        setLoading(false);
        return [];
      }
      if (showLoading) setLoading(true);
      try {
        const rows = await api.listComponents(tenantSelection(target), request.signal);
        if (request.signal.aborted || !request.isCurrent()) return null;
        setComponents(rows);
        setTenantKey(tenantKeyOf(target));
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

  const checkLatest = useCallback(async (): Promise<ComponentLatestSnapshot | null> => {
    if (checkingLatest) return null;
    setCheckingLatest(true);
    try {
      const snapshot = await api.checkLatestComponents();
      setLatestSnapshot(snapshot);
      return snapshot;
    } catch (cause) {
      onError(messageOf(cause));
      return null;
    } finally {
      setCheckingLatest(false);
    }
  }, [api, checkingLatest, onError]);

  const preserveNextError = useCallback(() => {
    preserveError.current = true;
  }, []);

  useEffect(() => {
    const latestOwner = latestRequest.current;
    const catalogOwner = catalogRequest.current;
    const request = latestOwner.begin();
    void api
      .latestComponents(request.signal)
      .then((snapshot) => {
        if (!request.signal.aborted && request.isCurrent()) setLatestSnapshot(snapshot);
      })
      .catch(() => {
        // A missing observation is an expected first-run state.
      })
      .finally(() => {
        if (request.isCurrent()) request.release();
      });
    return () => {
      latestOwner.cancel();
      catalogOwner.cancel();
    };
  }, [api]);

  return {
    checkingLatest,
    components,
    latestSnapshot,
    load,
    loading,
    preserveNextError,
    checkLatest,
    tenantKey,
  };
}
