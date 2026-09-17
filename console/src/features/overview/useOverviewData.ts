import { useCallback, useEffect, useRef, useState } from "react";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

const OVERVIEW_POLL_MS = 15000;

export function useOverviewData(
  api: Pick<OverviewApi, "loadOverview" | "loadTopology"> & {
    loadRequestsCount?: (signal?: AbortSignal) => Promise<number>;
  },
) {
  const [overview, setOverview] = useState<OverviewData | null>(null);
  const [topology, setTopology] = useState<TopologyData | null>(null);
  const [requestsTotal, setRequestsTotal] = useState<number | null>(null);
  const [overviewError, setOverviewError] = useState<string | null>(null);
  const [topologyError, setTopologyError] = useState<string | null>(null);
  const [overviewRefreshing, setOverviewRefreshing] = useState(false);
  const [topologyRefreshing, setTopologyRefreshing] = useState(false);
  const [uptimeTick, setUptimeTick] = useState(0);
  const [overviewLoadedAt, setOverviewLoadedAt] = useState(0);
  const overviewRequest = useRef(new LatestRequest());
  const topologyRequest = useRef(new LatestRequest());
  const requestsCountRequest = useRef(new LatestRequest());

  const loadOverview = useCallback(
    async (visibleRefresh = false) => {
      const request = overviewRequest.current.begin();
      if (visibleRefresh) setOverviewRefreshing(true);
      try {
        const value = await api.loadOverview(request.signal);
        if (request.signal.aborted || !request.isCurrent()) return;
        setOverview(value);
        setOverviewLoadedAt(Date.now());
        setUptimeTick(Date.now());
        setOverviewError(null);
      } catch (cause) {
        if (!request.signal.aborted) setOverviewError(messageOf(cause));
      } finally {
        if (request.isCurrent()) {
          request.release();
          if (visibleRefresh) setOverviewRefreshing(false);
        }
      }
    },
    [api],
  );

  const loadTopology = useCallback(
    async (visibleRefresh = false) => {
      const request = topologyRequest.current.begin();
      if (visibleRefresh) setTopologyRefreshing(true);
      try {
        const value = await api.loadTopology(request.signal);
        if (request.signal.aborted || !request.isCurrent()) return;
        setTopology(value);
        setTopologyError(null);
      } catch (cause) {
        if (!request.signal.aborted) setTopologyError(messageOf(cause));
      } finally {
        if (request.isCurrent()) {
          request.release();
          if (visibleRefresh) setTopologyRefreshing(false);
        }
      }
    },
    [api],
  );

  const loadRequestsTotal = useCallback(async () => {
    if (!api.loadRequestsCount) return;
    const request = requestsCountRequest.current.begin();
    try {
      const value = await api.loadRequestsCount(request.signal);
      if (request.signal.aborted || !request.isCurrent()) return;
      setRequestsTotal(value);
    } catch {
      // Quietly preserve previous value
    } finally {
      if (request.isCurrent()) request.release();
    }
  }, [api]);

  useEffect(() => {
    const overviewOwner = overviewRequest.current;
    const topologyOwner = topologyRequest.current;
    const requestsCountOwner = requestsCountRequest.current;
    // These calls start synchronization with external Service resources.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadOverview();
    void loadTopology();
    void loadRequestsTotal();
    const poll = window.setInterval(() => {
      if (document.visibilityState === "visible") {
        void loadOverview();
        void loadRequestsTotal();
      }
    }, OVERVIEW_POLL_MS);
    const tick = window.setInterval(() => setUptimeTick(Date.now()), 1000);
    return () => {
      window.clearInterval(poll);
      window.clearInterval(tick);
      overviewOwner.cancel();
      topologyOwner.cancel();
      requestsCountOwner.cancel();
    };
  }, [loadOverview, loadTopology, loadRequestsTotal]);

  const elapsedUptime = overview
    ? overview.service.uptime_seconds +
      Math.max(0, Math.floor((uptimeTick - overviewLoadedAt) / 1000))
    : 0;

  return {
    elapsedUptime,
    loadOverview,
    loadTopology,
    loadRequestsTotal,
    requestsTotal,
    overview,
    overviewError,
    overviewRefreshing,
    topology,
    topologyError,
    topologyRefreshing,
  };
}
