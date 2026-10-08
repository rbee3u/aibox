import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentKind } from "@/domain/agent";
import type { ConfigApi, ConfigListData } from "@/api/configs";
import {
  tenantSelectionValue,
  tenantSelectionFromValue,
  type TenantSelection,
} from "@/domain/tenant";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

/** Owns one Tenant-and-Agent Config catalog request lifecycle. */
export function useConfigCatalog(
  api: Pick<ConfigApi, "listConfigs">,
  tenant: TenantSelection,
  agent: AgentKind,
  onLoaded?: (catalog: ConfigListData) => void,
) {
  const tenantKey = tenantSelectionValue(tenant);
  const selectedTenant = useMemo(() => tenantSelectionFromValue(tenantKey), [tenantKey]);
  const onLoadedRef = useRef(onLoaded);
  useEffect(() => {
    onLoadedRef.current = onLoaded;
  }, [onLoaded]);
  const scopeKey = JSON.stringify([tenantKey, agent]);
  const [loaded, setLoaded] = useState<{ scopeKey: string; catalog: ConfigListData } | null>(null);
  const catalog = loaded?.scopeKey === scopeKey ? loaded.catalog : null;
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestOwner = useRef(new LatestRequest());

  const load = useCallback(
    async (kind: ConfigCatalogLoadKind = "initial") => {
      if (kind === "initial") setLoading(true);
      if (kind === "refresh") setRefreshing(true);
      return requestOwner.current.run((signal) => api.listConfigs(selectedTenant, agent, signal), {
        loaded: (value) => {
          onLoadedRef.current?.(value);
          setLoaded({ scopeKey, catalog: value });
          setError(null);
        },
        failed: (cause) => {
          if (!(cause instanceof DOMException)) setError(messageOf(cause));
        },
        settled: () => {
          if (kind === "initial") setLoading(false);
          if (kind === "refresh") setRefreshing(false);
        },
      });
    },
    [agent, api, scopeKey, selectedTenant],
  );

  useEffect(() => {
    // A Tenant or Agent selection change starts a fresh catalog lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoaded(null);
    setError(null);
    const owner = requestOwner.current;
    void load();
    return () => owner.cancel();
  }, [load]);

  return { catalog, loading, refreshing, error, load };
}
