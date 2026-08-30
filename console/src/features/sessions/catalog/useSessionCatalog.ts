import { useCallback, useEffect, useRef, useState } from "react";

import type { SessionApi } from "@/api/sessions";
import { aggregateSessionCatalog, splitSessionResults } from "@/features/sessions/sessionCatalog";
import {
  visibleSessionSource,
  type AggregatedSessionData,
  type SessionSource,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
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
  sources: SessionSource[];
}

/** Owns the cancellable multi-source Session catalog lifecycle. */
export function useSessionCatalog({
  abortDetailStream,
  api,
  clearInspection,
  inspectedSession,
  onSelectionReset,
  onSourceLifecycleReset,
  replaceCurrent,
  setError,
  sources,
}: SessionCatalogOptions) {
  const [data, setData] = useState<AggregatedSessionData | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const requestOwner = useRef(new LatestRequest());

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
        const results = await Promise.allSettled(
          sources.map(async (source) => {
            const result = await api.listSessions(source.tenant, source.agent, request.signal);
            return { result, source };
          }),
        );
        if (request.signal.aborted || !request.isCurrent()) return null;
        const { successes, failures } = splitSessionResults(results, sources);
        if (successes.length === 0 && failures.length > 0) {
          const failureText = failures
            .map(({ cause, source }) => `${visibleSessionSource(source)}: ${messageOf(cause)}`)
            .join("; ");
          setUnavailable(true);
          setError(`Couldn’t load Sessions: ${failureText}`);
          setData((current) =>
            kind === "refresh" && current ? current : { sessions: [], warnings: [], partial: true },
          );
          onSelectionReset();
          return null;
        }
        const result: AggregatedSessionData = aggregateSessionCatalog(successes, failures);
        setData(result);
        setError(null);
        setUnavailable(false);
        const inspected = inspectedSession();
        if (inspected) {
          const refreshed = result.sessions.find((row) => row.key === inspected.key);
          if (refreshed) replaceCurrent(refreshed);
          else clearInspection();
        }
        if (result.warnings.length > 0) {
          onSelectionReset();
        }
        return result;
      } catch (cause) {
        if (request.isCurrent() && !sessionRequestCancelled(cause, request.signal)) {
          setError(messageOf(cause));
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
    [api, clearInspection, inspectedSession, onSelectionReset, replaceCurrent, setError, sources],
  );

  useEffect(() => {
    const owner = requestOwner.current;
    // A source-filter change starts a fresh external catalog lifecycle.
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
