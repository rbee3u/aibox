import { useCallback, useEffect, useRef, useState } from "react";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { ConfigApi, ConfigListData } from "@/api/configs";
import type { TenantSelection } from "@/domain/tenant";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

/** Owns one Tenant-and-Agent Config catalog request lifecycle. */
export function useConfigCatalog(
  api: Pick<ConfigApi, "listConfigs">,
  tenant: TenantSelection,
  agent: CodingAgentKind,
  onLoaded?: (catalog: ConfigListData) => void,
) {
  const [catalog, setCatalog] = useState<ConfigListData | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestOwner = useRef(new LatestRequest());

  const load = useCallback(
    async (kind: ConfigCatalogLoadKind = "initial") => {
      const request = requestOwner.current.begin();
      if (kind === "initial") setLoading(true);
      if (kind === "refresh") setRefreshing(true);
      try {
        const value = await api.listConfigs(tenant, agent, request.signal);
        if (request.signal.aborted || !request.isCurrent()) return null;
        onLoaded?.(value);
        setCatalog(value);
        setError(null);
        return value;
      } catch (cause) {
        if (!(request.signal.aborted || cause instanceof DOMException)) setError(messageOf(cause));
        return null;
      } finally {
        if (request.isCurrent()) {
          request.release();
          if (kind === "initial") setLoading(false);
          if (kind === "refresh") setRefreshing(false);
        }
      }
    },
    [agent, api, onLoaded, tenant],
  );

  useEffect(() => {
    // A Tenant or Coding Agent selection change starts a fresh catalog lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setCatalog(null);
    setError(null);
    const owner = requestOwner.current;
    void load();
    return () => owner.cancel();
  }, [load]);

  return { catalog, loading, refreshing, error, load };
}
