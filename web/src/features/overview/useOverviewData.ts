import { useCallback, useEffect, useRef, useState } from "react";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

const OVERVIEW_POLL_MS = 15000;

export function useOverviewData(
  api: Pick<OverviewApi, "loadOverview" | "loadTopology" | "loadRequestsCount">,
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
      if (visibleRefresh) setOverviewRefreshing(true);
      await overviewRequest.current.run((signal) => api.loadOverview(signal), {
        loaded: (value) => {
          setOverview(value);
          setOverviewLoadedAt(Date.now());
          setUptimeTick(Date.now());
          setOverviewError(null);
        },
        failed: (cause) => setOverviewError(messageOf(cause)),
        settled: () => {
          if (visibleRefresh) setOverviewRefreshing(false);
        },
      });
    },
    [api],
  );

  const loadTopology = useCallback(
    async (visibleRefresh = false) => {
      if (visibleRefresh) setTopologyRefreshing(true);
      await topologyRequest.current.run((signal) => api.loadTopology(signal), {
        loaded: (value) => {
          setTopology(value);
          setTopologyError(null);
        },
        failed: (cause) => setTopologyError(messageOf(cause)),
        settled: () => {
          if (visibleRefresh) setTopologyRefreshing(false);
        },
      });
    },
    [api],
  );

  const loadRequestsTotal = useCallback(async () => {
    const load = api.loadRequestsCount?.bind(api);
    if (!load) return;
    await requestsCountRequest.current.run(load, {
      loaded: setRequestsTotal,
      failed: () => {}, // Preserve the previous count when a background read fails.
    });
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
    requestsTotal,
    overview,
    overviewError,
    overviewRefreshing,
    topology,
    topologyError,
    topologyRefreshing,
  };
}
