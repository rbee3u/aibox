import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type KeyboardEvent,
  type RefObject,
  type SetStateAction,
} from "react";

import type { Operation } from "@/api/operations";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
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
  type TopologyHealth,
  type TopologyMetrics,
  type TopologyNode,
  type TopologySearchResult,
} from "@/features/overview/topology/topologyModel";
import {
  useTopologyInteraction,
  type TopologyZoomMode,
} from "@/features/overview/topology/useTopologyInteraction";
import { useOverviewData } from "@/features/overview/useOverviewData";
import { messageOf } from "@/shared/lib/errors";
import type { ConsoleNavigate } from "@/shared/lib/navigation";

interface ControllerOptions {
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

/**
 * What the Overview page reads, grouped by what the page is showing.
 *
 * Overview has neither a catalog nor a detail pane, so it groups by its own three
 * concerns rather than borrowing the other four pages' names: the Service and its
 * Runtime Image, the topology tree and its viewport, and the attention summary
 * derived from both.
 */
export interface OverviewViewModel {
  service: {
    build: (force: boolean) => Promise<void>;
    buildDisabled: boolean;
    buildUnavailableReason: string | null;
    elapsedUptime: number;
    loadOverview: (visibleRefresh?: boolean) => Promise<void>;
    overview: OverviewData | null;
    overviewError: string | null;
    overviewRefreshing: boolean;
  };
  topology: {
    collapseAll: () => void;
    expandAll: () => void;
    expanded: Set<string>;
    filteredTree: TopologyNode | null;
    fitTopology: () => void;
    forcedExpanded: Set<string>;
    loadSessionSummary: (id: string, request: SessionRequest, force?: boolean) => Promise<void>;
    loadTopology: (visibleRefresh?: boolean) => Promise<void>;
    metrics: TopologyMetrics | null;
    navigateTree: (event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) => void;
    pageRef: RefObject<HTMLDivElement | null>;
    query: string;
    registerNode: (id: string, element: HTMLDivElement | null) => void;
    renderedActiveNode: string;
    resetZoom: () => void;
    sessionLoads: Record<string, SessionLoad>;
    setActiveNode: Dispatch<SetStateAction<string>>;
    setQuery: Dispatch<SetStateAction<string>>;
    toggleNode: (node: TopologyNode) => void;
    topology: TopologyData | null;
    topologyError: string | null;
    topologyRefreshing: boolean;
    topologySearch: TopologySearchResult;
    treeRef: RefObject<HTMLElement | null>;
    updateMetrics: (next: TopologyMetrics) => void;
    zoom: number;
    zoomIn: () => void;
    zoomMode: TopologyZoomMode;
    zoomOut: () => void;
  };
  attention: {
    attentionItems: AttentionItem[];
    attentionOnly: boolean;
    health: TopologyHealth | null;
    revealAttention: () => void;
    toggleAttention: () => void;
  };
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

  const viewModel: OverviewViewModel = {
    service: {
      build,
      buildDisabled,
      buildUnavailableReason,
      elapsedUptime,
      loadOverview,
      overview,
      overviewError,
      overviewRefreshing,
    },
    topology: {
      collapseAll: topologyInteraction.collapseAll,
      expandAll: topologyInteraction.expandAll,
      expanded,
      filteredTree,
      fitTopology: topologyInteraction.fit,
      forcedExpanded,
      loadSessionSummary,
      loadTopology,
      metrics: topologyInteraction.metrics,
      navigateTree: topologyInteraction.navigateTree,
      pageRef: topologyInteraction.pageRef,
      query,
      registerNode: topologyInteraction.registerNode,
      renderedActiveNode: topologyInteraction.activeNode,
      resetZoom: topologyInteraction.resetZoom,
      sessionLoads,
      setActiveNode: topologyInteraction.setActiveNode,
      setQuery,
      toggleNode: topologyInteraction.toggleNode,
      topology,
      topologyError,
      topologyRefreshing,
      topologySearch,
      treeRef: topologyInteraction.treeRef,
      updateMetrics: topologyInteraction.updateMetrics,
      zoom: topologyInteraction.zoom,
      zoomIn: topologyInteraction.zoomIn,
      zoomMode: topologyInteraction.zoomMode,
      zoomOut: topologyInteraction.zoomOut,
    },
    attention: {
      attentionItems,
      attentionOnly,
      health,
      revealAttention: () => {
        setAttentionOnly(true);
        window.requestAnimationFrame(topologyInteraction.reveal);
      },
      toggleAttention: () => setAttentionOnly((value) => !value),
    },
  };
  return viewModel;
}
