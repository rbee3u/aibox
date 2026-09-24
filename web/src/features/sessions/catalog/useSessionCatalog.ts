import { useCallback, useEffect, useRef, useState } from "react";

import type { SessionApi } from "@/api/sessions";
import { projectSessionCatalog } from "@/features/sessions/sessionCatalog";
import {
  sessionSource,
  type AggregatedSessionData,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import {
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelection,
} from "@/domain/tenant";
import type { AgentKind } from "@/domain/agent";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

interface SessionCatalogOptions {
  abortDetailStream: () => void;
  api: Pick<SessionApi, "listSessions">;
  clearInspection: () => void;
  inspectedSession: () => SourcedSession | null;
  onSelectionReset: () => void;
  onSourceLifecycleReset: () => void;
  replaceCurrent: (row: SourcedSession) => void;
  setError: (error: string | null) => void;
  tenant: TenantSelection;
  agent: AgentKind;
}

/** Owns the cancellable single-source Session catalog lifecycle. */
export function useSessionCatalog({
  abortDetailStream,
  api,
  clearInspection,
  inspectedSession,
  onSelectionReset,
  onSourceLifecycleReset,
  replaceCurrent,
  setError,
  tenant,
  agent,
}: SessionCatalogOptions) {
  const [data, setData] = useState<AggregatedSessionData | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const requestOwner = useRef(new LatestRequest());
  const tenantKey = tenantSelectionValue(tenant);

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
      const request = requestOwner.current.begin();
      if (kind === "refresh") {
        setLoading(false);
        setRefreshing(true);
      } else {
        setRefreshing(false);
        setLoading(true);
      }
      try {
        const tenantSelection = tenantSelectionFromValue(tenantKey);
        const result = await api.listSessions(tenantSelection, agent, request.signal);
        if (request.signal.aborted || !request.isCurrent()) return null;
        const source = sessionSource(tenantKey, agent);
        const aggregated = projectSessionCatalog(source, result);
        setData(aggregated);
        setError(null);
        setUnavailable(false);
        const inspected = inspectedSession();
        if (inspected) {
          const refreshed = aggregated.sessions.find((row) => row.key === inspected.key);
          if (refreshed) replaceCurrent(refreshed);
          else clearInspection();
        }
        if (aggregated.warnings.length > 0) {
          onSelectionReset();
        }
        return aggregated;
      } catch (cause) {
        if (request.isCurrent() && !sessionRequestCancelled(cause, request.signal)) {
          setUnavailable(true);
          setError(`Couldn’t load Sessions: ${messageOf(cause)}`);
          setData((current) =>
            kind === "refresh" && current ? current : { sessions: [], warnings: [], partial: true },
          );
          onSelectionReset();
        }
        return null;
      } finally {
        if (request.isCurrent()) {
          if (kind === "refresh") setRefreshing(false);
          else setLoading(false);
        }
        request.release();
      }
    },
    [
      agent,
      api,
      clearInspection,
      inspectedSession,
      onSelectionReset,
      replaceCurrent,
      setError,
      tenantKey,
    ],
  );

  useEffect(() => {
    const owner = requestOwner.current;
    // A filter change starts a fresh external catalog lifecycle.
    /* eslint-disable react-hooks/set-state-in-effect */
    clearInspection();
    reset();
    onSourceLifecycleReset();
    /* eslint-enable react-hooks/set-state-in-effect */
    void load();
    return () => {
      owner.cancel();
      abortDetailStream();
    };
  }, [abortDetailStream, clearInspection, load, onSourceLifecycleReset, reset]);

  return { data, load, loading, refreshing, removeSession, reset, unavailable };
}
