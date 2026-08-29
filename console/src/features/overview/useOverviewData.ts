import { useCallback, useEffect, useRef, useState } from "react";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

const OVERVIEW_POLL_MS = 15000;

export function useOverviewData(
  api: Pick<OverviewApi, "loadOverview" | "loadTopology">,
  onTopologyLoaded: (topology: TopologyData) => void,
) {
  const [overview, setOverview] = useState<OverviewData | null>(null);
  const [topology, setTopology] = useState<TopologyData | null>(null);
  const [overviewError, setOverviewError] = useState<string | null>(null);
  const [topologyError, setTopologyError] = useState<string | null>(null);
  const [overviewRefreshing, setOverviewRefreshing] = useState(false);
  const [topologyRefreshing, setTopologyRefreshing] = useState(false);
  const [uptimeTick, setUptimeTick] = useState(0);
  const [overviewLoadedAt, setOverviewLoadedAt] = useState(0);
  const overviewRequest = useRef(new LatestRequest());
  const topologyRequest = useRef(new LatestRequest());

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
        onTopologyLoaded(value);
      } catch (cause) {
        if (!request.signal.aborted) setTopologyError(messageOf(cause));
      } finally {
        if (request.isCurrent()) {
          request.release();
          if (visibleRefresh) setTopologyRefreshing(false);
        }
      }
    },
    [api, onTopologyLoaded],
  );

  useEffect(() => {
    const overviewOwner = overviewRequest.current;
    const topologyOwner = topologyRequest.current;
    // These calls start synchronization with external Service resources.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadOverview();
    void loadTopology();
    const poll = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadOverview();
    }, OVERVIEW_POLL_MS);
    const tick = window.setInterval(() => setUptimeTick(Date.now()), 1000);
    return () => {
      window.clearInterval(poll);
      window.clearInterval(tick);
      overviewOwner.cancel();
      topologyOwner.cancel();
    };
  }, [loadOverview, loadTopology]);

  const elapsedUptime = overview
    ? overview.service.uptime_seconds +
      Math.max(0, Math.floor((uptimeTick - overviewLoadedAt) / 1000))
    : 0;

  return {
    elapsedUptime,
    loadOverview,
    loadTopology,
    overview,
    overviewError,
    overviewRefreshing,
    reportOverviewError: setOverviewError,
    topology,
    topologyError,
    topologyRefreshing,
  };
}
