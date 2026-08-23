import { Info, Minus, Plus, RefreshCw } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { AgentIcon } from "./icons";
import { moduleIcons, resourceIcons, type ModuleId } from "./consoleIcons";
import {
  layoutTopology,
  sessionAnnouncement,
  targetHref,
  topologyPath,
  visibleTopology,
  type SessionLoad,
  type Tone,
  type TopologyLayoutNode,
  type TopologyMetrics,
  type TopologyNode,
  type TopologySearchResult,
  type TreeIcon,
} from "./overviewTopology";
import styles from "./OverviewPage.module.css";

const ComponentGroupIcon = resourceIcons.components;
const ComponentIcon = resourceIcons.component;
const ConfigsModuleIcon = moduleIcons.configs;
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
const ServiceIcon = resourceIcons.service;
const SessionsModuleIcon = moduleIcons.sessions;
const SessionIcon = resourceIcons.session;
type ConsoleNavigate = (module: ModuleId, query?: URLSearchParams) => void;

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
      <span className={styles.srOnly} aria-live="polite">
        {sessionAnnouncement(props.sessionLoads)}
      </span>
    </div>
  );
}
interface TopologyCanvasNodeProps {
  layoutNode: TopologyLayoutNode;
  active: boolean;
  traced: boolean;
  query: string;
  matched: boolean;
  contextual: boolean;
  forcedOpen: boolean;
  detailOpen: boolean;
  load?: SessionLoad;
  canvasWidth: number;
  canvasHeight: number;
  registerNode: (id: string, element: HTMLDivElement | null) => void;
  onFocus: (id: string) => void;
  onHover: (id: string | null) => void;
  onKeyDown: (event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) => void;
  onToggle: (node: TopologyNode) => void;
  onNavigate: ConsoleNavigate;
  onDetail: (id: string) => void;
  onCloseDetail: () => void;
  onRefreshSession: (node: TopologyNode) => void;
}
function TopologyCanvasNode(props: TopologyCanvasNodeProps) {
  const { layoutNode } = props;
  const { node } = layoutNode;
  const canInspect = Boolean(node.title);
  const dimmed = Boolean(props.query) && !props.matched && !props.contextual;
  const popoverAbove = layoutNode.y + layoutNode.height + 112 > props.canvasHeight;
  const popoverEnd = layoutNode.x + layoutNode.width + 250 > props.canvasWidth;
  const content = (
    <>
      <span className={styles.nodeIcon} data-icon={node.icon}>
        {treeIcon(node.icon)}
      </span>
      <span className={styles.nodeCopy}>
        <strong>
          <Highlighted value={node.label} query={props.query} />
        </strong>
        {node.detail && <small>{node.detail}</small>}
      </span>
      <StatusMark tone={node.tone} />
    </>
  );
  return (
    <div
      ref={(element) => props.registerNode(node.id, element)}
      className={`${styles.topologyNode} ${styles[layoutNode.kind]} ${styles[node.tone]} ${props.active ? styles.nodeActive : ""} ${props.traced ? styles.nodeTraced : ""} ${props.matched ? styles.nodeMatched : ""} ${dimmed ? styles.nodeDimmed : ""}`}
      style={{
        left: layoutNode.x,
        top: layoutNode.y,
        width: layoutNode.width,
        height: layoutNode.height,
      }}
      role="treeitem"
      aria-level={layoutNode.depth + 1}
      aria-posinset={layoutNode.position}
      aria-setsize={layoutNode.setSize}
      aria-expanded={layoutNode.branch ? layoutNode.open : undefined}
      tabIndex={props.active ? 0 : -1}
      data-node-id={node.id}
      data-node-kind={layoutNode.kind}
      onMouseEnter={() => props.onHover(node.id)}
      onMouseLeave={() => props.onHover(null)}
      onFocus={(event) => event.target === event.currentTarget && props.onFocus(node.id)}
      onKeyDown={(event) => props.onKeyDown(event, node)}
    >
      {node.parentId && <span className={styles.inputPort} aria-hidden="true" />}
      {node.target ? (
        <a
          className={styles.nodeSurface}
          href={targetHref(node.target)}
          tabIndex={-1}
          onClick={(event) => {
            event.preventDefault();
            props.onNavigate(node.target!.module, node.target!.query);
          }}
        >
          {content}
        </a>
      ) : (
        <div className={styles.nodeSurface}>{content}</div>
      )}
      {canInspect && (
        <button
          type="button"
          className={styles.detailButton}
          aria-label={`Show details for ${node.label}`}
          aria-expanded={props.detailOpen}
          data-topology-detail
          onMouseEnter={() => !props.detailOpen && props.onDetail(node.id)}
          onMouseLeave={(event) => {
            if (document.activeElement !== event.currentTarget) props.onCloseDetail();
          }}
          onFocus={() => !props.detailOpen && props.onDetail(node.id)}
          onBlur={props.onCloseDetail}
          onClick={() => props.onDetail(node.id)}
        >
          <Info size={12} />
        </button>
      )}
      {node.sessionRequest && props.load && props.load.state !== "loading" && (
        <button
          type="button"
          className={styles.sessionRefresh}
          aria-label={`Refresh ${node.label} summary`}
          title={`Refresh ${node.label} summary`}
          onClick={() => props.onRefreshSession(node)}
        >
          <RefreshCw size={12} />
        </button>
      )}
      {props.detailOpen && node.title && (
        <div
          className={`${styles.nodePopover} ${popoverAbove ? styles.popoverAbove : ""} ${popoverEnd ? styles.popoverEnd : ""}`}
          role="tooltip"
          data-topology-detail
        >
          <strong>{node.label}</strong>
          <span>{node.title}</span>
        </div>
      )}
      {layoutNode.branch && node.id !== "service" ? (
        <button
          type="button"
          className={styles.disclosure}
          tabIndex={-1}
          aria-label={`${layoutNode.open ? "Collapse" : "Expand"} ${node.label}`}
          aria-expanded={layoutNode.open}
          disabled={props.forcedOpen}
          title={props.forcedOpen ? "Clear the active filter to collapse this branch" : undefined}
          onClick={() => {
            props.onFocus(node.id);
            props.onToggle(node);
          }}
        >
          {layoutNode.open ? <Minus size={13} /> : <Plus size={13} />}
          {!layoutNode.open && node.children.length > 0 && (
            <span className={styles.collapsedCount}>{node.children.length}</span>
          )}
        </button>
      ) : layoutNode.branch ? (
        <span className={styles.outputPort} aria-hidden="true" />
      ) : null}
    </div>
  );
}
function StatusMark({ tone }: { tone: Tone }) {
  const label = statusLabel(tone);
  return (
    <span className={`${styles.statusMark} ${styles[tone]}`} title={label} aria-label={label} />
  );
}
function statusLabel(tone: Tone): string {
  switch (tone) {
    case "good":
      return "Healthy";
    case "warning":
      return "Needs attention";
    case "error":
      return "Error";
    case "neutral":
      return "Neutral";
  }
}
function Highlighted({ value, query }: { value: string; query: string }) {
  if (!query) return value;
  const index = value.toLocaleLowerCase().indexOf(query.toLocaleLowerCase());
  if (index < 0) return value;
  return (
    <>
      {value.slice(0, index)}
      <mark>{value.slice(index, index + query.length)}</mark>
      {value.slice(index + query.length)}
    </>
  );
}
function treeIcon(icon: TreeIcon): ReactNode {
  switch (icon) {
    case "service":
      return <ServiceIcon size={17} />;
    case "host":
      return <HostTenantIcon size={16} />;
    case "tenant":
      return <ManagedTenantIcon size={16} />;
    case "codex":
      return <AgentIcon agent="codex" size={16} />;
    case "claude":
      return <AgentIcon agent="claude" size={16} />;
    case "current":
      return <CurrentConfigIcon size={15} />;
    case "configs":
      return <ConfigsModuleIcon size={15} />;
    case "config":
      return <NamedConfigIcon size={15} />;
    case "sessions":
      return <SessionsModuleIcon size={15} />;
    case "session-summary":
      return <SessionIcon size={14} />;
    case "components":
      return <ComponentGroupIcon size={15} />;
    case "component":
      return <ComponentIcon size={15} />;
  }
}
