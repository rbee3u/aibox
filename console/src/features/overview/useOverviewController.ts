import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";

import type { Operation } from "@/api/operations";
import type { OverviewApi, TopologyData } from "@/api/overview";
import {
  buildDisabledReason,
  buildTopologyTree,
  clampZoom,
  collectBranchIds,
  collectVisibleNodes,
  defaultExpansion,
  emptyFilteredRoot,
  filterByAttention,
  firstComponentAttentionTarget,
  firstConfigAttentionTarget,
  fitTopologyZoom,
  requestAttentionDetail,
  searchTopology,
  structuralIds,
  summarizeTopology,
  type AttentionItem,
  type SessionLoad,
  type SessionRequest,
  type TopologyMetrics,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import { useOverviewData } from "@/features/overview/useOverviewData";
import { messageOf } from "@/shared/lib/errors";
import type { ConsoleNavigate } from "@/shared/lib/navigation";

const ZOOM_STEP = 0.1;
const MOBILE_CANVAS_WIDTH = 760;

interface TopologyAnchorSnapshot {
  id: string;
  left: number;
  top: number;
}

type TopologyZoomMode = "initial" | "fit" | "manual";

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
  const [activeNode, setActiveNode] = useState("service");
  const [query, setQuery] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [sessionLoads, setSessionLoads] = useState<Record<string, SessionLoad>>({});
  const [topologyZoom, setTopologyZoom] = useState(1);
  const [topologyZoomMode, setTopologyZoomMode] = useState<TopologyZoomMode>("initial");
  const [topologyMetrics, setTopologyMetrics] = useState<TopologyMetrics | null>(null);
  const sessionRequests = useRef(new Map<string, AbortController>());
  const initializedTopology = useRef(false);
  const pendingExpansionAnchor = useRef<TopologyAnchorSnapshot | null>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLElement>(null);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
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
  const renderedActiveNode = visibleNodes.some((entry) => entry.node.id === activeNode)
    ? activeNode
    : (filteredTree?.id ?? "service");
  const updateTopologyMetrics = useCallback((next: TopologyMetrics) => {
    setTopologyMetrics((current) =>
      current?.layoutWidth === next.layoutWidth && current.viewportWidth === next.viewportWidth
        ? current
        : next,
    );
  }, []);
  useEffect(() => {
    if (activeNode !== renderedActiveNode) setActiveNode(renderedActiveNode);
  }, [activeNode, renderedActiveNode]);
  useLayoutEffect(() => {
    const pending = pendingExpansionAnchor.current;
    if (!pending) return;
    pendingExpansionAnchor.current = null;
    const element = nodeRefs.current.get(pending.id);
    const viewport = element?.closest<HTMLElement>("[data-topology-viewport]");
    const page = pageRef.current;
    if (!element || !viewport || !page) return;
    const after = element.getBoundingClientRect();
    viewport.scrollLeft += after.left - pending.left;
    page.scrollTop += after.top - pending.top;
  }, [visibleNodes]);
  useEffect(() => {
    if (!topologyMetrics || topologyZoomMode === "manual") return;
    if (topologyZoomMode === "initial" && topologyMetrics.viewportWidth <= MOBILE_CANVAS_WIDTH) {
      setTopologyZoom(1);
      setTopologyZoomMode("manual");
      return;
    }
    setTopologyZoom(fitTopologyZoom(topologyMetrics.layoutWidth, topologyMetrics.viewportWidth));
    if (topologyZoomMode === "initial") setTopologyZoomMode("fit");
  }, [topologyMetrics, topologyZoomMode]);
  useEffect(() => {
    if (!query.trim() || !topologySearch.firstMatch) return;
    const frame = window.requestAnimationFrame(() => {
      scrollTopologyElement(nodeRefs.current.get(topologySearch.firstMatch!), {
        block: "nearest",
        inline: "center",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [query, topologySearch.firstMatch, topologyZoom, visibleNodes]);
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

  function captureExpansionAnchor(id: string) {
    const before = nodeRefs.current.get(id)?.getBoundingClientRect();
    pendingExpansionAnchor.current = before ? { id, left: before.left, top: before.top } : null;
  }

  function toggleNode(node: TopologyNode) {
    if (node.children.length === 0 && !node.sessionRequest) return;
    const opening = !expanded.has(node.id);
    captureExpansionAnchor(node.id);
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(node.id)) next.add(node.id);
      return next;
    });
    if (opening && node.sessionRequest) void loadSessionSummary(node.id, node.sessionRequest);
  }

  function replaceExpansion(next: Set<string>) {
    captureExpansionAnchor("service");
    setExpanded(next);
  }

  function focusNode(id: string) {
    setActiveNode(id);
    const element = nodeRefs.current.get(id);
    element?.focus({ preventScroll: true });
    scrollTopologyElement(element, { block: "nearest", inline: "nearest" });
  }

  function navigateTree(event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) {
    if (event.target !== event.currentTarget) return;
    const index = visibleNodes.findIndex((value) => value.node.id === node.id);
    const current = visibleNodes[index];
    if (!current) return;
    let destination: string | null = null;
    const open = node.id === "service" || expanded.has(node.id) || forcedExpanded.has(node.id);
    const branch = node.children.length > 0 || Boolean(node.sessionRequest);
    switch (event.key) {
      case "ArrowDown":
        destination = visibleNodes[index + 1]?.node.id ?? null;
        break;
      case "ArrowUp":
        destination = visibleNodes[index - 1]?.node.id ?? null;
        break;
      case "Home":
        destination = visibleNodes[0]?.node.id ?? null;
        break;
      case "End":
        destination = visibleNodes.at(-1)?.node.id ?? null;
        break;
      case "ArrowRight":
        if (!branch) break;
        if (!open) toggleNode(node);
        else
          destination =
            visibleNodes[index + 1]?.parentId === node.id ? visibleNodes[index + 1].node.id : null;
        break;
      case "ArrowLeft":
        if (node.id !== "service" && branch && open && !forcedExpanded.has(node.id))
          toggleNode(node);
        else destination = current.parentId;
        break;
      case " ":
        if (node.id !== "service" && branch) toggleNode(node);
        break;
      case "Enter":
        if (node.target) onNavigate(node.target.module, node.target.query);
        else if (node.id !== "service" && branch) toggleNode(node);
        break;
      default:
        return;
    }
    event.preventDefault();
    if (destination) focusNode(destination);
  }

  function changeTopologyZoom(value: number) {
    setTopologyZoomMode("manual");
    setTopologyZoom(clampZoom(value));
  }

  return {
    attentionItems,
    attentionOnly,
    build,
    buildDisabled,
    buildUnavailableReason,
    collapseAll: () => replaceExpansion(new Set()),
    elapsedUptime,
    expandAll: () => topology && replaceExpansion(structuralIds(topology)),
    expanded,
    filteredTree,
    fitTopology: () => {
      if (!topologyMetrics) return;
      setTopologyZoomMode("fit");
      setTopologyZoom(fitTopologyZoom(topologyMetrics.layoutWidth, topologyMetrics.viewportWidth));
    },
    forcedExpanded,
    health,
    loadOverview,
    loadSessionSummary,
    loadTopology,
    navigateTree,
    overview,
    overviewError,
    overviewRefreshing,
    pageRef,
    query,
    registerNode: (id: string, element: HTMLDivElement | null) => {
      if (element) nodeRefs.current.set(id, element);
      else nodeRefs.current.delete(id);
    },
    renderedActiveNode,
    resetZoom: () => changeTopologyZoom(1),
    revealAttention: () => {
      setAttentionOnly(true);
      window.requestAnimationFrame(() =>
        scrollTopologyElement(treeRef.current, { behavior: "smooth" }),
      );
    },
    sessionLoads,
    setActiveNode,
    setQuery,
    toggleAttention: () => setAttentionOnly((value) => !value),
    toggleNode,
    topology,
    topologyError,
    topologyMetrics,
    topologyRefreshing,
    topologySearch,
    topologyZoom,
    topologyZoomMode,
    treeRef,
    updateTopologyMetrics,
    zoomIn: () => changeTopologyZoom(topologyZoom + ZOOM_STEP),
    zoomOut: () => changeTopologyZoom(topologyZoom - ZOOM_STEP),
  };
}

function scrollTopologyElement(
  element: Element | null | undefined,
  options: ScrollIntoViewOptions,
) {
  if (element && typeof element.scrollIntoView === "function") element.scrollIntoView(options);
}
