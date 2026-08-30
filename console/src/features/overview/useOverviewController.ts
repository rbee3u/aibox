import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Operation } from "@/api/operations";
import type { OverviewApi, TopologyData } from "@/api/overview";
import {
  buildDisabledReason,
  buildTopologyTree,
  collectBranchIds,
  collectVisibleNodes,
  defaultExpansion,
  emptyFilteredRoot,
  filterByAttention,
  firstComponentAttentionTarget,
  firstConfigAttentionTarget,
  requestAttentionDetail,
  searchTopology,
  structuralIds,
  summarizeTopology,
  type AttentionItem,
  type SessionLoad,
  type SessionRequest,
} from "@/features/overview/topology/topologyModel";
import { useTopologyInteraction } from "@/features/overview/topology/useTopologyInteraction";
import { useOverviewData } from "@/features/overview/useOverviewData";
import { messageOf } from "@/shared/lib/errors";
import type { ConsoleNavigate } from "@/shared/lib/navigation";

interface ControllerOptions {
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

export function useOverviewController({
  api,
  operation,
  onNavigate,
  onOperation,
}: ControllerOptions) {
  const [buildPosting, setBuildPosting] = useState(false);
  const [ownedBuild, setOwnedBuild] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [sessionLoads, setSessionLoads] = useState<Record<string, SessionLoad>>({});
  const sessionRequests = useRef(new Map<string, AbortController>());
  const initializedTopology = useRef(false);
  const onTopologyLoaded = useCallback((value: TopologyData) => {
    const structural = structuralIds(value);
    const firstLoad = !initializedTopology.current;
    if (firstLoad) {
      setExpanded(defaultExpansion(value));
      initializedTopology.current = true;
    } else {
      setExpanded((current) => new Set([...current].filter((id) => structural.has(id))));
    }
  }, []);
  const {
    elapsedUptime,
    loadOverview,
    loadTopology,
    overview,
    overviewError,
    overviewRefreshing,
    reportOverviewError,
    topology,
    topologyError,
    topologyRefreshing,
  } = useOverviewData(api, onTopologyLoaded);
  useEffect(() => {
    const pendingSessionRequests = sessionRequests.current;
    return () => {
      for (const controller of pendingSessionRequests.values()) controller.abort();
    };
  }, []);
  useEffect(() => {
    if (!ownedBuild || operation?.id !== ownedBuild || operation.state === "running") return;
    setOwnedBuild(null);
    void loadOverview();
  }, [loadOverview, operation, ownedBuild]);
  const loadSessionSummary = useCallback(
    async (id: string, request: SessionRequest, force = false) => {
      if (!force && sessionLoads[id]?.state === "loaded") return;
      sessionRequests.current.get(id)?.abort();
      const controller = new AbortController();
      sessionRequests.current.set(id, controller);
      setSessionLoads((current) => ({ ...current, [id]: { state: "loading" } }));
      try {
        const data = await api.loadSessionSummary(request.tenant, request.agent, controller.signal);
        if (controller.signal.aborted || sessionRequests.current.get(id) !== controller) return;
        setSessionLoads((current) => ({ ...current, [id]: { state: "loaded", data } }));
      } catch (cause) {
        if (!controller.signal.aborted) {
          setSessionLoads((current) => ({
            ...current,
            [id]: { state: "error", error: messageOf(cause) },
          }));
        }
      } finally {
        if (sessionRequests.current.get(id) === controller) sessionRequests.current.delete(id);
      }
    },
    [api, sessionLoads],
  );
  const tree = useMemo(
    () => (topology ? buildTopologyTree(topology, sessionLoads) : null),
    [sessionLoads, topology],
  );
  const health = useMemo(() => (topology ? summarizeTopology(topology) : null), [topology]);
  const attentionTree = useMemo(() => {
    if (!tree) return null;
    if (!attentionOnly) return tree;
    return filterByAttention(tree) ?? emptyFilteredRoot(tree, "No resources need attention");
  }, [attentionOnly, tree]);
  const topologySearch = useMemo(
    () => searchTopology(attentionTree, query.trim()),
    [attentionTree, query],
  );
  const filteredTree = useMemo(() => {
    if (!attentionTree) return null;
    if (!query.trim() || topologySearch.matches.size > 0) return attentionTree;
    return emptyFilteredRoot(
      attentionTree,
      attentionOnly ? "No matching resources need attention" : "No resources match this filter",
    );
  }, [attentionOnly, attentionTree, query, topologySearch.matches.size]);
  const forcedExpanded = useMemo(() => {
    const result = new Set<string>();
    if (attentionOnly && filteredTree) collectBranchIds(filteredTree, result);
    if (query.trim()) {
      for (const id of topologySearch.context) result.add(id);
    }
    return result;
  }, [attentionOnly, filteredTree, query, topologySearch.context]);
  const visibleNodes = useMemo(
    () => (filteredTree ? collectVisibleNodes(filteredTree, expanded, forcedExpanded) : []),
    [expanded, filteredTree, forcedExpanded],
  );
  const topologyInteraction = useTopologyInteraction({
    expanded,
    filteredTree,
    firstMatch: topologySearch.firstMatch,
    forcedExpanded,
    onLoadSessionSummary: (id, request) => void loadSessionSummary(id, request),
    onNavigate,
    query,
    setExpanded,
    topology,
    visibleNodes,
  });
  const operationRunning = operation?.state === "running";
  const buildDisabled = buildPosting || operationRunning || overview?.docker.status !== "available";
  const buildUnavailableReason = buildPosting
    ? "Build request is being submitted."
    : buildDisabled
      ? buildDisabledReason(overview, operation)
      : null;
  const attentionItems = useMemo<AttentionItem[]>(() => {
    const items: AttentionItem[] = [];
    if (overviewError)
      items.push({ label: "Service status", detail: overviewError, tone: "error" });
    if (overview?.docker.status === "unavailable")
      items.push({
        label: "Docker",
        detail: overview.docker.error ?? "Docker is unavailable.",
        tone: "error",
      });
    if (overview && overview.runtime_image.status !== "built")
      items.push({
        label: "Runtime Image",
        detail: overview.runtime_image.detail ?? "Build the Runtime Image before starting a Run.",
        tone: overview.runtime_image.status === "missing" ? "warning" : "error",
      });
    if (overview?.host_available === false)
      items.push({
        label: "Host Tenant",
        detail: "The Host Home is unavailable.",
        tone: "warning",
        target: { module: "tenants", query: new URLSearchParams("tenant=host") },
      });
    if (health?.configAttention)
      items.push({
        label: "Configs",
        detail: `${health.configAttention} Named Config${health.configAttention === 1 ? " needs" : "s need"} attention.`,
        tone: health.configErrors ? "error" : "warning",
        target: topology ? firstConfigAttentionTarget(topology) : { module: "configs" },
      });
    if (health?.componentAttention)
      items.push({
        label: "Components",
        detail: `${health.componentAttention} Component${health.componentAttention === 1 ? " needs" : "s need"} attention.`,
        tone: health.componentErrors ? "error" : "warning",
        target: topology ? firstComponentAttentionTarget(topology) : { module: "tenants" },
      });
    if (overview?.requests.error || overview?.requests.warning)
      items.push({
        label: "Requests",
        detail: requestAttentionDetail(overview),
        tone: overview.requests.error ? "error" : "warning",
        target: { module: "requests" },
      });
    if (topologyError)
      items.push({ label: "Resource inspection", detail: topologyError, tone: "error" });
    return items;
  }, [health, overview, overviewError, topology, topologyError]);

  async function build(force: boolean) {
    setBuildPosting(true);
    try {
      const value = await api.buildImage(force);
      setOwnedBuild(value.id);
      onOperation(value);
      reportOverviewError(null);
    } catch (cause) {
      reportOverviewError(messageOf(cause));
    } finally {
      setBuildPosting(false);
    }
  }

  return {
    attentionItems,
    attentionOnly,
    build,
    buildDisabled,
    buildUnavailableReason,
    collapseAll: topologyInteraction.collapseAll,
    elapsedUptime,
    expandAll: topologyInteraction.expandAll,
    expanded,
    filteredTree,
    fitTopology: topologyInteraction.fit,
    forcedExpanded,
    health,
    loadOverview,
    loadSessionSummary,
    loadTopology,
    navigateTree: topologyInteraction.navigateTree,
    overview,
    overviewError,
    overviewRefreshing,
    pageRef: topologyInteraction.pageRef,
    query,
    registerNode: topologyInteraction.registerNode,
    renderedActiveNode: topologyInteraction.activeNode,
    resetZoom: topologyInteraction.resetZoom,
    revealAttention: () => {
      setAttentionOnly(true);
      window.requestAnimationFrame(topologyInteraction.reveal);
    },
    sessionLoads,
    setActiveNode: topologyInteraction.setActiveNode,
    setQuery,
    toggleAttention: () => setAttentionOnly((value) => !value),
    toggleNode: topologyInteraction.toggleNode,
    topology,
    topologyError,
    topologyMetrics: topologyInteraction.metrics,
    topologyRefreshing,
    topologySearch,
    topologyZoom: topologyInteraction.zoom,
    topologyZoomMode: topologyInteraction.zoomMode,
    treeRef: topologyInteraction.treeRef,
    updateTopologyMetrics: topologyInteraction.updateMetrics,
    zoomIn: topologyInteraction.zoomIn,
    zoomOut: topologyInteraction.zoomOut,
  };
}
