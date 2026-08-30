import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type KeyboardEvent,
  type SetStateAction,
} from "react";

import type { TopologyData } from "@/api/overview";
import {
  clampZoom,
  fitTopologyZoom,
  structuralIds,
  type SessionRequest,
  type TopologyMetrics,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import type { ConsoleNavigate } from "@/shared/lib/navigation";

const ZOOM_STEP = 0.1;
const MOBILE_CANVAS_WIDTH = 760;

interface TopologyAnchorSnapshot {
  id: string;
  left: number;
  top: number;
}

export type TopologyZoomMode = "initial" | "fit" | "manual";

interface TopologyInteractionOptions {
  expanded: Set<string>;
  filteredTree: TopologyNode | null;
  firstMatch: string | null;
  forcedExpanded: Set<string>;
  onLoadSessionSummary: (id: string, request: SessionRequest) => void;
  onNavigate: ConsoleNavigate;
  query: string;
  setExpanded: Dispatch<SetStateAction<Set<string>>>;
  topology: TopologyData | null;
  visibleNodes: Array<{ node: TopologyNode; parentId: string | null }>;
}

export function useTopologyInteraction({
  expanded,
  filteredTree,
  firstMatch,
  forcedExpanded,
  onLoadSessionSummary,
  onNavigate,
  query,
  setExpanded,
  topology,
  visibleNodes,
}: TopologyInteractionOptions) {
  const [activeNode, setActiveNode] = useState("service");
  const [zoom, setZoom] = useState(1);
  const [zoomMode, setZoomMode] = useState<TopologyZoomMode>("initial");
  const [metrics, setMetrics] = useState<TopologyMetrics | null>(null);
  const pendingExpansionAnchor = useRef<TopologyAnchorSnapshot | null>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLElement>(null);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
  const renderedActiveNode = visibleNodes.some((entry) => entry.node.id === activeNode)
    ? activeNode
    : (filteredTree?.id ?? "service");

  const updateMetrics = useCallback((next: TopologyMetrics) => {
    setMetrics((current) =>
      current?.layoutWidth === next.layoutWidth && current.viewportWidth === next.viewportWidth
        ? current
        : next,
    );
  }, []);

  useEffect(() => {
    // The rendered tree can invalidate the roving focus anchor.
    // eslint-disable-next-line react-hooks/set-state-in-effect
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
    if (!metrics || zoomMode === "manual") return;
    if (zoomMode === "initial" && metrics.viewportWidth <= MOBILE_CANVAS_WIDTH) {
      // Narrow first render intentionally stays at 100% instead of entering Fit mode.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setZoom(1);
      setZoomMode("manual");
      return;
    }
    setZoom(fitTopologyZoom(metrics.layoutWidth, metrics.viewportWidth));
    if (zoomMode === "initial") setZoomMode("fit");
  }, [metrics, zoomMode]);

  useEffect(() => {
    if (!query.trim() || !firstMatch) return;
    const frame = window.requestAnimationFrame(() => {
      scrollTopologyElement(nodeRefs.current.get(firstMatch), {
        block: "nearest",
        inline: "center",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [firstMatch, query, visibleNodes, zoom]);

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
    if (opening && node.sessionRequest) onLoadSessionSummary(node.id, node.sessionRequest);
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

  function changeZoom(value: number) {
    setZoomMode("manual");
    setZoom(clampZoom(value));
  }

  return {
    activeNode: renderedActiveNode,
    collapseAll: () => replaceExpansion(new Set()),
    expandAll: () => topology && replaceExpansion(structuralIds(topology)),
    fit: () => {
      if (!metrics) return;
      setZoomMode("fit");
      setZoom(fitTopologyZoom(metrics.layoutWidth, metrics.viewportWidth));
    },
    metrics,
    navigateTree,
    pageRef,
    registerNode: (id: string, element: HTMLDivElement | null) => {
      if (element) nodeRefs.current.set(id, element);
      else nodeRefs.current.delete(id);
    },
    resetZoom: () => changeZoom(1),
    reveal: () => scrollTopologyElement(treeRef.current, { behavior: "smooth" }),
    setActiveNode,
    toggleNode,
    treeRef,
    updateMetrics,
    zoom,
    zoomIn: () => changeZoom(zoom + ZOOM_STEP),
    zoomMode,
    zoomOut: () => changeZoom(zoom - ZOOM_STEP),
  };
}

function scrollTopologyElement(
  element: Element | null | undefined,
  options: ScrollIntoViewOptions,
) {
  if (element && typeof element.scrollIntoView === "function") element.scrollIntoView(options);
}
