import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

import {
  layoutTopology,
  sessionAnnouncement,
  topologyPath,
  visibleTopology,
  type SessionLoad,
  type TopologyMetrics,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import { TopologyCanvasNode } from "@/features/overview/topology/TopologyCanvasNode";
import { TopologyInspector } from "@/features/overview/topology/TopologyInspector";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import styles from "@/features/overview/OverviewPage.module.css";

interface TopologyCanvasProps {
  root: TopologyNode;
  expanded: Set<string>;
  forcedExpanded: Set<string>;
  activeNode: string;
  selectedNode: TopologyNode | null;
  zoom: number;
  sessionLoads: Record<string, SessionLoad>;
  onMetricsChange: (metrics: TopologyMetrics) => void;
  registerNode: (id: string, element: HTMLDivElement | null) => void;
  onFocus: (id: string) => void;
  onSelect: (id: string) => void;
  onCloseInspector: () => void;
  onNavigate: ConsoleNavigate;
  onKeyDown: (event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) => void;
  onToggle: (node: TopologyNode) => void;
  onRefreshSession: (node: TopologyNode) => void;
}
export function TopologyCanvas(props: TopologyCanvasProps) {
  const { onMetricsChange } = props;
  const { onCloseInspector, selectedNode } = props;
  const viewportRef = useRef<HTMLDivElement>(null);
  const previousZoom = useRef(props.zoom);
  const [viewportWidth, setViewportWidth] = useState(0);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const [popoverStyle, setPopoverStyle] = useState<{ top: number; left: number } | undefined>();
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
    if (!selectedNode) {
      // The next layout pass removes the portal; clearing its stale position is external UI state.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setPopoverStyle(undefined);
      return;
    }
    const reposition = () => {
      const element = [...document.querySelectorAll<HTMLElement>("[data-node-id]")].find(
        (candidate) => candidate.dataset.nodeId === selectedNode.id,
      );
      if (!element) {
        setPopoverStyle(undefined);
        return;
      }
      const rect = element.getBoundingClientRect();
      // JSDOM and detached layout probes report a zero rect; keep the popup
      // renderable there while real off-screen nodes remain temporarily hidden.
      const hasLayout = rect.width > 0 || rect.height > 0;
      const inViewport =
        !hasLayout ||
        (rect.bottom > 0 &&
          rect.top < window.innerHeight &&
          rect.right > 0 &&
          rect.left < window.innerWidth);
      if (!inViewport) {
        setPopoverStyle(undefined);
        return;
      }
      const width = 300;
      const height = 260;
      const gap = 12;
      const left =
        rect.right + width + gap <= window.innerWidth ? rect.right + gap : rect.left - width - gap;
      let top = rect.top + rect.height / 2 - height / 2;
      if (top < 12 && rect.bottom + gap + height <= window.innerHeight) {
        top = rect.bottom + gap;
      } else if (top + height > window.innerHeight - 12 && rect.top - gap - height >= 12) {
        top = rect.top - gap - height;
      }
      top = Math.max(12, Math.min(window.innerHeight - height - 12, top));
      setPopoverStyle({ top, left: Math.max(12, left) });
    };
    reposition();
    const viewport = viewportRef.current;
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    viewport?.addEventListener("scroll", reposition);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
      viewport?.removeEventListener("scroll", reposition);
    };
  }, [layout.nodes, props.zoom, selectedNode]);
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
    if (!selectedNode) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target;
      if (
        target instanceof Element &&
        (target.closest("[data-topology-popover]") || target.closest("[data-node-id]"))
      )
        return;
      onCloseInspector();
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onCloseInspector();
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [onCloseInspector, selectedNode]);
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
                return (
                  <path
                    key={edge.id}
                    d={edge.path}
                    className={`${styles.topologyEdge} ${styles[edge.tone]} ${active ? styles.edgeActive : ""}`}
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
                  forcedOpen={
                    props.forcedExpanded.has(layoutNode.node.id) &&
                    !props.expanded.has(layoutNode.node.id)
                  }
                  load={props.sessionLoads[layoutNode.node.id]}
                  registerNode={props.registerNode}
                  onFocus={props.onFocus}
                  onSelect={props.onSelect}
                  onHover={setHoveredNode}
                  onKeyDown={props.onKeyDown}
                  onToggle={props.onToggle}
                  onRefreshSession={props.onRefreshSession}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
      {selectedNode &&
        popoverStyle &&
        createPortal(
          <TopologyInspector
            node={selectedNode}
            onClose={onCloseInspector}
            onNavigate={props.onNavigate}
            style={popoverStyle}
          />,
          document.body,
        )}
      <span className="srOnly" aria-live="polite">
        {sessionAnnouncement(props.sessionLoads)}
      </span>
    </div>
  );
}
