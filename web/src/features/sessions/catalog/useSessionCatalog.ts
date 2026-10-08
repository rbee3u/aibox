import { useCallback, useRef, useState } from "react";

import type { SessionApi } from "@/api/sessions";
import {
  projectSessionCatalog,
  type SessionCatalogResult,
} from "@/features/sessions/sessionCatalog";
import { sessionSource, type AggregatedSessionData } from "@/features/sessions/sessionSource";
import {
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelection,
} from "@/domain/tenant";
import type { AgentKind } from "@/domain/agent";
import { LatestRequest } from "@/shared/lib/latestRequest";

interface SessionCatalogOptions {
  api: Pick<SessionApi, "listSessions">;
  tenant: TenantSelection;
  agent: AgentKind;
  onResult: (result: SessionCatalogResult) => void;
}

/** Owns list resources; publishes results only while their request lease is current. */
export function useSessionCatalog({ api, tenant, agent, onResult }: SessionCatalogOptions) {
  const [data, setData] = useState<AggregatedSessionData | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const requestOwner = useRef(new LatestRequest());
  const tenantKey = tenantSelectionValue(tenant);
  const cancel = useCallback(() => requestOwner.current.cancel(), []);

  const reset = useCallback(() => {
    setData(null);
    setUnavailable(false);
  }, []);

  const removeSession = useCallback((key: string) => {
    setData((current) =>
      current
        ? { ...current, sessions: current.sessions.filter((session) => session.key !== key) }
        : current,
    );
  }, []);

  const load = useCallback(
    async (kind: "initial" | "refresh" = "initial"): Promise<AggregatedSessionData | null> => {
      if (kind === "refresh") {
        setLoading(false);
        setRefreshing(true);
      } else {
        setRefreshing(false);
        setLoading(true);
      }
      let projected: AggregatedSessionData | null = null;
      await requestOwner.current.run(
        (signal) => api.listSessions(tenantSelectionFromValue(tenantKey), agent, signal),
        {
          loaded: (result) => {
            const aggregated = projectSessionCatalog(sessionSource(tenantKey, agent), result);
            projected = aggregated;
            setData(aggregated);
            setUnavailable(false);
            onResult({ kind: "loaded", data: aggregated });
          },
          failed: (cause) => {
            if (cause instanceof DOMException && cause.name === "AbortError") return;
            setUnavailable(true);
            setData((current) =>
              kind === "refresh" && current
                ? current
                : { sessions: [], warnings: [], partial: true },
            );
            onResult({ kind: "failed", cause });
          },
          settled: () => {
            if (kind === "refresh") setRefreshing(false);
            else setLoading(false);
          },
        },
      );
      return projected;
    },
    [agent, api, onResult, tenantKey],
  );

  return { cancel, data, load, loading, refreshing, removeSession, reset, unavailable };
}
