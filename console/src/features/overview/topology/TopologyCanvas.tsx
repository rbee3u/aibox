import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import type { ConsoleNavigate } from "@/shared/lib/navigation";
import {
  layoutTopology,
  sessionAnnouncement,
  topologyPath,
  visibleTopology,
  type SessionLoad,
  type TopologyMetrics,
  type TopologyNode,
  type TopologySearchResult,
} from "@/features/overview/topology/topologyModel";
import { TopologyCanvasNode } from "@/features/overview/topology/TopologyCanvasNode";
import styles from "@/features/overview/OverviewPage.module.css";

interface TopologyCanvasProps {
  root: TopologyNode;
  expanded: Set<string>;
  forcedExpanded: Set<string>;
  activeNode: string;
  query: string;
  search: TopologySearchResult;
  zoom: number;
  sessionLoads: Record<string, SessionLoad>;
  onMetricsChange: (metrics: TopologyMetrics) => void;
  registerNode: (id: string, element: HTMLDivElement | null) => void;
  onFocus: (id: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) => void;
  onToggle: (node: TopologyNode) => void;
  onNavigate: ConsoleNavigate;
  onRefreshSession: (node: TopologyNode) => void;
}
export function TopologyCanvas(props: TopologyCanvasProps) {
  const { onMetricsChange } = props;
  const viewportRef = useRef<HTMLDivElement>(null);
  const previousZoom = useRef(props.zoom);
  const [viewportWidth, setViewportWidth] = useState(0);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const [detailNode, setDetailNode] = useState<string | null>(null);
  const visibleRoot = useMemo(
    () => visibleTopology(props.root, props.expanded, props.forcedExpanded),
    [props.expanded, props.forcedExpanded, props.root],
  );
  const layout = useMemo(
    () => layoutTopology(visibleRoot, viewportWidth || 1024),
    [viewportWidth, visibleRoot],
  );
  const tracedNode = hoveredNode ?? props.activeNode;
  const tracedPath = useMemo(() => topologyPath(props.root, tracedNode), [props.root, tracedNode]);
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const updateWidth = (width: number) => setViewportWidth(Math.max(320, Math.floor(width)));
    if (typeof ResizeObserver === "undefined") {
      updateWidth(viewport.clientWidth || 1024);
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width;
      if (width) updateWidth(width);
    });
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!viewportWidth) return;
    onMetricsChange({ layoutWidth: layout.width, viewportWidth });
  }, [layout.width, onMetricsChange, viewportWidth]);
  useLayoutEffect(() => {
    const oldZoom = previousZoom.current;
    previousZoom.current = props.zoom;
    if (oldZoom === props.zoom) return;
    const viewport = viewportRef.current;
    const page = viewport?.closest<HTMLElement>("[data-overview-scroll]");
    if (!viewport || !page) return;
    const active = layout.nodes.find((entry) => entry.node.id === props.activeNode);
    const viewportRect = viewport.getBoundingClientRect();
    const pageRect = page.getBoundingClientRect();
    const anchorX = active
      ? active.x + active.width / 2
      : (viewport.scrollLeft + viewport.clientWidth / 2) / oldZoom;
    const anchorY = active
      ? active.y + active.height / 2
      : Math.min(
          layout.height,
          Math.max(0, (pageRect.top + page.clientHeight / 2 - viewportRect.top) / oldZoom),
        );
    const delta = props.zoom - oldZoom;
    viewport.scrollLeft += anchorX * delta;
    page.scrollTop += anchorY * delta;
  }, [layout.height, layout.nodes, props.activeNode, props.zoom]);
  useEffect(() => {
    if (!detailNode) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-topology-detail]")) return;
      setDetailNode(null);
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") setDetailNode(null);
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [detailNode]);
  return (
    <div className={styles.canvasShell}>
      <div
        ref={viewportRef}
        className={styles.topologyViewport}
        data-topology-viewport
        data-scroll-axis="horizontal"
      >
        <div
          className={styles.scaledCanvasFrame}
          style={{ width: layout.width * props.zoom, height: layout.height * props.zoom }}
        >
          <div
            className={styles.topologyCanvas}
            style={{
              width: layout.width,
              height: layout.height,
              transform: `scale(${props.zoom})`,
            }}
          >
            <svg
              className={styles.topologyEdges}
              width={layout.width}
              height={layout.height}
              viewBox={`0 0 ${layout.width} ${layout.height}`}
              aria-hidden="true"
            >
              {layout.edges.map((edge) => {
                const active = tracedPath.has(edge.parentId) && tracedPath.has(edge.childId);
                const contextual =
                  !props.query ||
                  props.search.matches.has(edge.childId) ||
                  props.search.context.has(edge.childId);
                return (
                  <path
                    key={edge.id}
                    d={edge.path}
                    className={`${styles.topologyEdge} ${styles[edge.tone]} ${active ? styles.edgeActive : ""} ${!contextual ? styles.edgeDimmed : ""}`}
                    data-edge={`${edge.parentId}->${edge.childId}`}
                  />
                );
              })}
            </svg>
            <div className={styles.topologyTree} role="tree" aria-label="Tenant resource topology">
              {layout.nodes.map((layoutNode) => (
                <TopologyCanvasNode
                  key={layoutNode.node.id}
                  layoutNode={layoutNode}
                  active={props.activeNode === layoutNode.node.id}
                  traced={tracedPath.has(layoutNode.node.id)}
                  query={props.query}
                  matched={props.search.matches.has(layoutNode.node.id)}
                  contextual={props.search.context.has(layoutNode.node.id)}
                  forcedOpen={
                    props.forcedExpanded.has(layoutNode.node.id) &&
                    !props.expanded.has(layoutNode.node.id)
                  }
                  detailOpen={detailNode === layoutNode.node.id}
                  load={props.sessionLoads[layoutNode.node.id]}
                  canvasWidth={layout.width}
                  canvasHeight={layout.height}
                  registerNode={props.registerNode}
                  onFocus={props.onFocus}
                  onHover={setHoveredNode}
                  onKeyDown={props.onKeyDown}
                  onToggle={props.onToggle}
                  onNavigate={props.onNavigate}
                  onDetail={setDetailNode}
                  onCloseDetail={() => setDetailNode(null)}
                  onRefreshSession={props.onRefreshSession}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
      <span className="srOnly" aria-live="polite">
        {sessionAnnouncement(props.sessionLoads)}
      </span>
    </div>
  );
}
