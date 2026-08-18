import {
  AlertTriangle,
  Box,
  ChevronsDownUp,
  ChevronsUpDown,
  Hammer,
  HardDrive,
  Image,
  Info,
  LoaderCircle,
  Minus,
  Network,
  Plus,
  RefreshCw,
  Scan,
  Search,
  Server,
  ShieldAlert,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  ControlApi,
  formatBytes,
  scopeQuery,
  type Agent,
  type ComponentRow,
  type Operation,
  type OverviewData,
  type Scope,
  type SessionSummaryData,
  type TopologyAgent,
  type TopologyData,
  type TopologyTenant,
} from "./controlApi";
import { AgentIcon } from "./icons";
import { moduleIcons, type ModuleId } from "./moduleIcons";
import { resourceIcons } from "./resourceIcons";
import { formatTimestamp } from "./utils";
import styles from "./OverviewPage.module.css";

const OVERVIEW_POLL_MS = 15_000;
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 1.5;
const ZOOM_STEP = 0.1;
const MOBILE_CANVAS_WIDTH = 760;
const ComponentGroupIcon = resourceIcons.components;
const ComponentIcon = resourceIcons.component;
const ConfigsModuleIcon = moduleIcons.configs;
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
const RequestsModuleIcon = moduleIcons.requests;
const ServiceIcon = resourceIcons.service;
const SessionsModuleIcon = moduleIcons.sessions;
const SessionIcon = resourceIcons.session;
const TenantsModuleIcon = moduleIcons.tenants;

export type ConsoleNavigate = (module: ModuleId, query?: URLSearchParams) => void;

interface OverviewPageProps {
  api: ControlApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

type Tone = "good" | "neutral" | "warning" | "error";
type TreeIcon =
  | "service"
  | "host"
  | "tenant"
  | "codex"
  | "claude"
  | "current"
  | "configs"
  | "config"
  | "sessions"
  | "session-summary"
  | "components"
  | "component";

interface NavigationTarget {
  module: ModuleId;
  query?: URLSearchParams;
}

interface SessionRequest {
  scope: Scope;
  agent: Agent;
}

interface TopologyNode {
  id: string;
  parentId: string | null;
  label: string;
  detail?: string;
  title?: string;
  icon: TreeIcon;
  tone: Tone;
  target?: NavigationTarget;
  sessionRequest?: SessionRequest;
  children: TopologyNode[];
}

type TopologyNodeKind = "entity" | "group" | "leaf";

interface VisibleTopologyNode {
  node: TopologyNode;
  children: VisibleTopologyNode[];
  depth: number;
  open: boolean;
  branch: boolean;
  position: number;
  setSize: number;
}

interface TopologyLayoutNode extends VisibleTopologyNode {
  x: number;
  y: number;
  width: number;
  height: number;
  kind: TopologyNodeKind;
}

interface TopologyLayoutEdge {
  id: string;
  parentId: string;
  childId: string;
  path: string;
  tone: Tone;
}

interface TopologyLayout {
  width: number;
  height: number;
  nodes: TopologyLayoutNode[];
  edges: TopologyLayoutEdge[];
}

interface TopologySearchResult {
  matches: Set<string>;
  context: Set<string>;
  firstMatch: string | null;
}

interface SessionLoad {
  state: "loading" | "loaded" | "error";
  data?: SessionSummaryData;
  error?: string;
}

interface TopologyHealth {
  configTotal: number;
  configAttention: number;
  configErrors: number;
  componentInstalled: number;
  componentAttention: number;
  componentErrors: number;
}

interface TopologyMetrics {
  layoutWidth: number;
  viewportWidth: number;
}

interface TopologyAnchorSnapshot {
  id: string;
  left: number;
  top: number;
}

type TopologyZoomMode = "initial" | "fit" | "manual";

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

export function OverviewPage({ api, operation, onNavigate, onOperation }: OverviewPageProps) {
  const [overview, setOverview] = useState<OverviewData | null>(null);
  const [topology, setTopology] = useState<TopologyData | null>(null);
  const [overviewError, setOverviewError] = useState<string | null>(null);
  const [topologyError, setTopologyError] = useState<string | null>(null);
  const [overviewRefreshing, setOverviewRefreshing] = useState(false);
  const [topologyRefreshing, setTopologyRefreshing] = useState(false);
  const [buildPosting, setBuildPosting] = useState(false);
  const [ownedBuild, setOwnedBuild] = useState<string | null>(null);
  const [uptimeTick, setUptimeTick] = useState(Date.now());
  const [overviewLoadedAt, setOverviewLoadedAt] = useState(Date.now());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [activeNode, setActiveNode] = useState("service");
  const [query, setQuery] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [sessionLoads, setSessionLoads] = useState<Record<string, SessionLoad>>({});
  const [topologyZoom, setTopologyZoom] = useState(1);
  const [topologyZoomMode, setTopologyZoomMode] = useState<TopologyZoomMode>("initial");
  const [topologyMetrics, setTopologyMetrics] = useState<TopologyMetrics | null>(null);
  const overviewRequest = useRef<AbortController | null>(null);
  const topologyRequest = useRef<AbortController | null>(null);
  const sessionRequests = useRef(new Map<string, AbortController>());
  const initializedTopology = useRef(false);
  const pendingExpansionAnchor = useRef<TopologyAnchorSnapshot | null>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLElement>(null);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());

  const loadOverview = useCallback(
    async (visibleRefresh = false) => {
      overviewRequest.current?.abort();
      const controller = new AbortController();
      overviewRequest.current = controller;
      if (visibleRefresh) setOverviewRefreshing(true);
      try {
        const value = await api.get<OverviewData>("/_aibox/api/overview", controller.signal);
        if (controller.signal.aborted || overviewRequest.current !== controller) return;
        setOverview(value);
        setOverviewLoadedAt(Date.now());
        setUptimeTick(Date.now());
        setOverviewError(null);
      } catch (cause) {
        if (!controller.signal.aborted) setOverviewError(messageOf(cause));
      } finally {
        if (overviewRequest.current === controller) {
          overviewRequest.current = null;
          if (visibleRefresh) setOverviewRefreshing(false);
        }
      }
    },
    [api],
  );

  const loadTopology = useCallback(
    async (visibleRefresh = false) => {
      topologyRequest.current?.abort();
      const controller = new AbortController();
      topologyRequest.current = controller;
      if (visibleRefresh) setTopologyRefreshing(true);
      try {
        const value = await api.get<TopologyData>("/_aibox/api/topology", controller.signal);
        if (controller.signal.aborted || topologyRequest.current !== controller) return;
        setTopology(value);
        setTopologyError(null);
        const structural = structuralIds(value);
        const firstLoad = !initializedTopology.current;
        if (firstLoad) {
          setExpanded(defaultExpansion(value));
          initializedTopology.current = true;
        } else {
          setExpanded((current) => new Set([...current].filter((id) => structural.has(id))));
        }
      } catch (cause) {
        if (!controller.signal.aborted) setTopologyError(messageOf(cause));
      } finally {
        if (topologyRequest.current === controller) {
          topologyRequest.current = null;
          if (visibleRefresh) setTopologyRefreshing(false);
        }
      }
    },
    [api],
  );

  useEffect(() => {
    const pendingSessionRequests = sessionRequests.current;
    void loadOverview();
    void loadTopology();
    const poll = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadOverview();
    }, OVERVIEW_POLL_MS);
    const tick = window.setInterval(() => setUptimeTick(Date.now()), 1_000);
    return () => {
      window.clearInterval(poll);
      window.clearInterval(tick);
      overviewRequest.current?.abort();
      topologyRequest.current?.abort();
      for (const controller of pendingSessionRequests.values()) controller.abort();
    };
  }, [loadOverview, loadTopology]);

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
      const params = scopeQuery(request.scope);
      params.set("agent", request.agent);
      try {
        const data = await api.get<SessionSummaryData>(
          `/_aibox/api/sessions/summary?${params}`,
          controller.signal,
        );
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

  const elapsedUptime = overview
    ? overview.service.uptime_seconds +
      Math.max(0, Math.floor((uptimeTick - overviewLoadedAt) / 1000))
    : 0;
  const operationRunning = operation?.state === "running";
  const buildDisabled = buildPosting || operationRunning || overview?.docker.status !== "available";

  async function build(force: boolean) {
    setBuildPosting(true);
    try {
      const value = await api.post<Operation>("/_aibox/api/operations/build", { force });
      setOwnedBuild(value.id);
      onOperation(value);
      setOverviewError(null);
    } catch (cause) {
      setOverviewError(messageOf(cause));
    } finally {
      setBuildPosting(false);
    }
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

  function captureExpansionAnchor(id: string) {
    const before = nodeRefs.current.get(id)?.getBoundingClientRect();
    pendingExpansionAnchor.current = before ? { id, left: before.left, top: before.top } : null;
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

  function revealAttention() {
    setAttentionOnly(true);
    window.requestAnimationFrame(() =>
      scrollTopologyElement(treeRef.current, { behavior: "smooth" }),
    );
  }

  function changeTopologyZoom(value: number) {
    setTopologyZoomMode("manual");
    setTopologyZoom(clampZoom(value));
  }

  function fitTopology() {
    if (!topologyMetrics) return;
    setTopologyZoomMode("fit");
    setTopologyZoom(fitTopologyZoom(topologyMetrics.layoutWidth, topologyMetrics.viewportWidth));
  }

  return (
    <div ref={pageRef} className={styles.page} data-overview-scroll data-scroll-axis="vertical">
      {overviewError && <ErrorBanner message={overviewError} />}
      <section className={styles.summarySection} aria-label="Service status">
        <SectionHeading
          eyebrow="Operational status"
          title="Key facts"
          action={
            <IconButton
              label={overviewRefreshing ? "Refreshing status" : "Refresh status"}
              disabled={overviewRefreshing}
              onClick={() => void loadOverview(true)}
            >
              <RefreshCw className={overviewRefreshing ? styles.spinning : undefined} size={16} />
            </IconButton>
          }
        />
        <div className={styles.factGrid}>
          <Fact
            icon={<Server size={18} />}
            label="Service"
            value={overview ? "Running" : "Loading"}
            detail={overview ? formatDuration(elapsedUptime) : "Connecting"}
            tone="good"
          />
          <Fact
            icon={<TenantsModuleIcon size={18} />}
            label="Managed Tenants"
            value={overview?.managed_tenants ?? "—"}
            detail={
              overview ? `Host ${overview.host_available ? "available" : "unavailable"}` : "Loading"
            }
            tone={overview?.host_available === false ? "warning" : "neutral"}
            onClick={() => onNavigate("tenants")}
          />
          <Fact
            icon={<ConfigsModuleIcon size={18} />}
            label="Config health"
            value={
              health
                ? attentionValue(health.configAttention)
                : topologyError
                  ? "Unknown"
                  : "Loading"
            }
            detail={
              health
                ? `${health.configTotal} Named Configs`
                : (topologyError ?? "Inspecting topology")
            }
            tone={healthTone(health?.configAttention, health?.configErrors, topologyError)}
            onClick={health?.configAttention ? revealAttention : undefined}
          />
          <Fact
            icon={<ComponentGroupIcon size={18} />}
            label="Component health"
            value={
              health
                ? attentionValue(health.componentAttention)
                : topologyError
                  ? "Unknown"
                  : "Loading"
            }
            detail={
              health
                ? `${health.componentInstalled} installed`
                : (topologyError ?? "Inspecting topology")
            }
            tone={healthTone(health?.componentAttention, health?.componentErrors, topologyError)}
            onClick={health?.componentAttention ? revealAttention : undefined}
          />
          <Fact
            icon={<RequestsModuleIcon size={18} />}
            label="Requests"
            value={overview?.requests.total ?? "—"}
            detail={overview ? requestDetail(overview) : "Loading"}
            tone={
              overview?.requests.error
                ? "error"
                : overview?.requests.warning
                  ? "warning"
                  : "neutral"
            }
            onClick={() => onNavigate("requests")}
          />
        </div>
        <div className={styles.metadataStrip}>
          <Metadata
            icon={<Box size={14} />}
            label="Version"
            value={overview?.service.version ?? "—"}
          />
          <Metadata
            icon={<Network size={14} />}
            label="Listen"
            value={overview?.service.listen ?? "—"}
            mono
          />
          <Metadata
            icon={<HardDrive size={14} />}
            label="aibox Root"
            value={overview?.service.aibox_root ?? "—"}
            mono
            wide
          />
        </div>
      </section>

      <section className={styles.runtimeSection} aria-labelledby="runtime-title">
        <SectionHeading eyebrow="Docker execution" title="Runtime" id="runtime-title" />
        <div className={styles.runtimeGrid}>
          <RuntimeStatus
            icon={<Server size={18} />}
            label="Docker"
            value={capitalize(overview?.docker.status ?? "checking")}
            detail={overview?.docker.error ?? "Docker CLI and daemon"}
            tone={overview?.docker.status === "available" ? "good" : overview ? "error" : "neutral"}
          />
          <RuntimeStatus
            icon={<Image size={18} />}
            label="Runtime Image"
            value={capitalize(overview?.runtime_image.status ?? "checking")}
            detail={overview?.runtime_image.reference ?? "Resolving image"}
            tone={imageTone(overview?.runtime_image.status)}
          />
          <dl className={styles.imageMetadata}>
            <div>
              <dt>Image ID</dt>
              <dd title={overview?.runtime_image.id ?? undefined}>
                {shortImageId(overview?.runtime_image.id)}
              </dd>
            </div>
            <div>
              <dt>Created</dt>
              <dd>
                {overview?.runtime_image.created_at
                  ? formatTimestamp(overview.runtime_image.created_at)
                  : "—"}
              </dd>
            </div>
            <div>
              <dt>Size</dt>
              <dd>
                {overview?.runtime_image.size_bytes == null
                  ? "—"
                  : formatBytes(overview.runtime_image.size_bytes)}
              </dd>
            </div>
          </dl>
          <div className={styles.runtimeActions}>
            {operationRunning && (
              <span className={styles.operationState} title={operation.kind}>
                <LoaderCircle className={styles.spinning} size={14} /> {operation.kind}
              </span>
            )}
            <button
              className={styles.primaryButton}
              type="button"
              disabled={buildDisabled}
              title={
                buildDisabled
                  ? buildDisabledReason(overview, operation)
                  : "Build Runtime Image using Docker cache"
              }
              onClick={() => void build(false)}
            >
              <Hammer size={15} /> Build
            </button>
            <button
              type="button"
              disabled={buildDisabled}
              title={
                buildDisabled
                  ? buildDisabledReason(overview, operation)
                  : "Re-run every layer without cache and pull a fresh base image"
              }
              onClick={() => void build(true)}
            >
              <RefreshCw size={15} /> Build without cache
            </button>
          </div>
        </div>
        {overview?.runtime_image.detail && (
          <div className={styles.runtimeNotice} role="status">
            <AlertTriangle size={15} /> {overview.runtime_image.detail}
          </div>
        )}
      </section>

      <section ref={treeRef} className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyHeading}>
          <SectionHeading
            eyebrow="Persistent identities"
            title="Resource topology"
            id="topology-title"
          />
          <span>{topology ? `${topology.tenants.length} Tenants` : "Loading topology"}</span>
        </div>
        <div className={styles.topologyToolbar}>
          <label className={styles.searchField}>
            <Search size={15} aria-hidden="true" />
            <span className={styles.srOnly}>Filter topology</span>
            <input
              type="search"
              placeholder="Filter resources"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          {query.trim() && (
            <span className={styles.matchCount} aria-live="polite">
              {topologySearch.matches.size} matched
            </span>
          )}
          <button
            type="button"
            className={attentionOnly ? styles.filterActive : undefined}
            aria-pressed={attentionOnly}
            onClick={() => setAttentionOnly((value) => !value)}
          >
            <ShieldAlert size={15} /> Needs attention
          </button>
          <IconButton
            label="Expand topology (Session summaries remain on demand)"
            disabled={!topology}
            onClick={() => topology && replaceExpansion(structuralIds(topology))}
          >
            <ChevronsUpDown size={16} />
          </IconButton>
          <IconButton
            label="Collapse all Tenant branches"
            disabled={!topology}
            onClick={() => replaceExpansion(new Set())}
          >
            <ChevronsDownUp size={16} />
          </IconButton>
          <IconButton
            label={topologyRefreshing ? "Refreshing topology" : "Refresh topology"}
            disabled={topologyRefreshing}
            onClick={() => void loadTopology(true)}
          >
            <RefreshCw className={topologyRefreshing ? styles.spinning : undefined} size={16} />
          </IconButton>
          <div className={styles.zoomControls} aria-label="Topology zoom controls">
            <IconButton
              label="Zoom out"
              disabled={!topologyMetrics || topologyZoom <= MIN_ZOOM}
              onClick={() => changeTopologyZoom(topologyZoom - ZOOM_STEP)}
            >
              <Minus size={15} />
            </IconButton>
            <button
              type="button"
              className={styles.zoomValue}
              disabled={!topologyMetrics}
              title="Reset topology zoom to 100%"
              aria-label={`Reset topology zoom to 100% (currently ${Math.round(topologyZoom * 100)}%)`}
              onClick={() => changeTopologyZoom(1)}
            >
              {Math.round(topologyZoom * 100)}%
            </button>
            <IconButton
              label="Zoom in"
              disabled={!topologyMetrics || topologyZoom >= MAX_ZOOM}
              onClick={() => changeTopologyZoom(topologyZoom + ZOOM_STEP)}
            >
              <Plus size={15} />
            </IconButton>
            <IconButton
              label="Fit topology to width"
              disabled={!topologyMetrics}
              aria-pressed={topologyZoomMode === "fit"}
              onClick={fitTopology}
            >
              <Scan size={15} />
            </IconButton>
          </div>
        </div>
        {topologyError && <ErrorBanner message={`Topology unavailable: ${topologyError}`} local />}
        {!filteredTree && !topologyError && (
          <div className={styles.treeLoading}>
            <LoaderCircle className={styles.spinning} size={20} /> Inspecting Tenant state
          </div>
        )}
        {filteredTree && (
          <TopologyCanvas
            root={filteredTree}
            expanded={expanded}
            forcedExpanded={forcedExpanded}
            activeNode={renderedActiveNode}
            query={query.trim()}
            search={topologySearch}
            zoom={topologyZoom}
            sessionLoads={sessionLoads}
            onMetricsChange={updateTopologyMetrics}
            registerNode={(id, element) => {
              if (element) nodeRefs.current.set(id, element);
              else nodeRefs.current.delete(id);
            }}
            onFocus={setActiveNode}
            onKeyDown={navigateTree}
            onToggle={toggleNode}
            onNavigate={onNavigate}
            onRefreshSession={(node) =>
              node.sessionRequest && void loadSessionSummary(node.id, node.sessionRequest, true)
            }
          />
        )}
      </section>
    </div>
  );
}

function SectionHeading({
  eyebrow,
  title,
  id,
  action,
}: {
  eyebrow: string;
  title: string;
  id?: string;
  action?: ReactNode;
}) {
  return (
    <div className={styles.sectionHeading}>
      <div>
        <span>{eyebrow}</span>
        <h2 id={id}>{title}</h2>
      </div>
      {action}
    </div>
  );
}

function IconButton({
  label,
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button className={styles.iconButton} type="button" title={label} aria-label={label} {...props}>
      {children}
    </button>
  );
}

function Fact({
  icon,
  label,
  value,
  detail,
  tone,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  value: ReactNode;
  detail: string;
  tone: Tone;
  onClick?: () => void;
}) {
  const content = (
    <>
      <span className={styles.factLabel}>
        {icon}
        {label}
      </span>
      <strong>{value}</strong>
      <small title={detail}>{detail}</small>
    </>
  );
  return onClick ? (
    <button type="button" className={`${styles.fact} ${styles[tone]}`} onClick={onClick}>
      {content}
    </button>
  ) : (
    <div className={`${styles.fact} ${styles[tone]}`}>{content}</div>
  );
}

function Metadata({
  icon,
  label,
  value,
  mono = false,
  wide = false,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  mono?: boolean;
  wide?: boolean;
}) {
  return (
    <div className={`${styles.metadata} ${wide ? styles.metadataWide : ""}`}>
      {icon}
      <span>{label}</span>
      <code className={mono ? styles.mono : undefined} title={value}>
        {value}
      </code>
    </div>
  );
}

function RuntimeStatus({
  icon,
  label,
  value,
  detail,
  tone,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  detail: string;
  tone: Tone;
}) {
  return (
    <div className={styles.runtimeStatus}>
      <span className={`${styles.statusIcon} ${styles[tone]}`}>{icon}</span>
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
        <small title={detail}>{detail}</small>
      </div>
    </div>
  );
}

function ErrorBanner({ message, local = false }: { message: string; local?: boolean }) {
  return (
    <div className={local ? styles.localError : styles.errorBanner} role="alert">
      <AlertTriangle size={16} /> <span>{message}</span>
    </div>
  );
}

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

function TopologyCanvas(props: TopologyCanvasProps) {
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

function sessionAnnouncement(loads: Record<string, SessionLoad>): string {
  const loading = Object.values(loads).filter((load) => load.state === "loading").length;
  if (loading) return `Discovering ${loading} Session ${loading === 1 ? "summary" : "summaries"}`;
  const latest = Object.values(loads).at(-1);
  if (!latest) return "";
  if (latest.state === "error") return "Session summary unavailable";
  return latest.data ? `${latest.data.count} Sessions discovered` : "";
}

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(value * 10) / 10));
}

function scrollTopologyElement(
  element: Element | null | undefined,
  options: ScrollIntoViewOptions,
) {
  if (element && typeof element.scrollIntoView === "function") element.scrollIntoView(options);
}

function fitTopologyZoom(canvasWidth: number, viewportWidth: number): number {
  return clampZoom((viewportWidth - 32) / canvasWidth);
}

function visibleTopology(
  node: TopologyNode,
  expanded: ReadonlySet<string>,
  forcedExpanded: ReadonlySet<string>,
  depth = 0,
  position = 1,
  setSize = 1,
): VisibleTopologyNode {
  const branch = node.children.length > 0 || Boolean(node.sessionRequest);
  const open = node.id === "service" || expanded.has(node.id) || forcedExpanded.has(node.id);
  const children = open
    ? node.children.map((child, index) =>
        visibleTopology(
          child,
          expanded,
          forcedExpanded,
          depth + 1,
          index + 1,
          node.children.length,
        ),
      )
    : [];
  return { node, children, depth, open, branch, position, setSize };
}

function layoutTopology(root: VisibleTopologyNode, availableWidth: number): TopologyLayout {
  const PADDING_X = 32;
  const PADDING_Y = 28;
  const MIN_LEVEL_GAP = 72;
  const MAX_LEVEL_GAP = 220;
  const SUBTREE_GAP = 20;
  const levels: VisibleTopologyNode[][] = [];
  const visitLevels = (entry: VisibleTopologyNode) => {
    (levels[entry.depth] ??= []).push(entry);
    for (const child of entry.children) visitLevels(child);
  };
  visitLevels(root);
  const levelWidths = levels.map((entries) =>
    Math.max(...entries.map((entry) => topologyNodeSize(entry.node.icon).width)),
  );
  const widthWithoutGaps = levelWidths.reduce((total, width) => total + width, PADDING_X * 2);
  const gapCount = Math.max(0, levelWidths.length - 1);
  const levelGap = gapCount
    ? Math.min(
        MAX_LEVEL_GAP,
        Math.max(MIN_LEVEL_GAP, (availableWidth - widthWithoutGaps) / gapCount),
      )
    : 0;
  const levelX: number[] = [];
  let nextX = PADDING_X;
  for (let depth = 0; depth < levelWidths.length; depth += 1) {
    levelX.push(nextX);
    nextX += levelWidths[depth] + levelGap;
  }

  const subtreeHeights = new Map<string, number>();
  const measure = (entry: VisibleTopologyNode): number => {
    const ownHeight = topologyNodeSize(entry.node.icon).height;
    if (entry.children.length === 0) {
      subtreeHeights.set(entry.node.id, ownHeight);
      return ownHeight;
    }
    const childHeight =
      entry.children.reduce((total, child) => total + measure(child), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    const height = Math.max(ownHeight, childHeight);
    subtreeHeights.set(entry.node.id, height);
    return height;
  };
  const rootHeight = measure(root);
  const nodes: TopologyLayoutNode[] = [];
  const place = (entry: VisibleTopologyNode, top: number) => {
    const size = topologyNodeSize(entry.node.icon);
    const subtreeHeight = subtreeHeights.get(entry.node.id) ?? size.height;
    nodes.push({
      ...entry,
      x: levelX[entry.depth],
      y: top + (subtreeHeight - size.height) / 2,
      width: size.width,
      height: size.height,
      kind: size.kind,
    });
    if (entry.children.length === 0) return;
    const childrenHeight =
      entry.children.reduce((total, child) => total + (subtreeHeights.get(child.node.id) ?? 0), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    let childTop = top + (subtreeHeight - childrenHeight) / 2;
    for (const child of entry.children) {
      place(child, childTop);
      childTop += (subtreeHeights.get(child.node.id) ?? 0) + SUBTREE_GAP;
    }
  };
  place(root, PADDING_Y);
  const nodeById = new Map(nodes.map((node) => [node.node.id, node]));
  const edges = nodes.flatMap((child) => {
    if (!child.node.parentId) return [];
    const parent = nodeById.get(child.node.parentId);
    if (!parent) return [];
    const startX = parent.x + parent.width;
    const startY = parent.y + parent.height / 2;
    const endX = child.x;
    const endY = child.y + child.height / 2;
    const curve = Math.max(28, (endX - startX) / 2);
    return [
      {
        id: `${parent.node.id}->${child.node.id}`,
        parentId: parent.node.id,
        childId: child.node.id,
        path: `M ${startX} ${startY} C ${startX + curve} ${startY}, ${endX - curve} ${endY}, ${endX} ${endY}`,
        tone: child.node.tone,
      },
    ];
  });
  const contentWidth = nextX - levelGap + PADDING_X;
  return {
    width: Math.max(availableWidth, contentWidth),
    height: Math.max(420, rootHeight + PADDING_Y * 2),
    nodes,
    edges,
  };
}

function topologyNodeSize(icon: TreeIcon): {
  width: number;
  height: number;
  kind: TopologyNodeKind;
} {
  if (["service", "host", "tenant", "codex", "claude"].includes(icon)) {
    return { width: 184, height: 58, kind: "entity" };
  }
  if (["config", "session-summary", "component"].includes(icon)) {
    return { width: 160, height: 38, kind: "leaf" };
  }
  return { width: 168, height: 46, kind: "group" };
}

function topologyPath(root: TopologyNode, target: string): Set<string> {
  const path = new Set<string>();
  const visit = (node: TopologyNode): boolean => {
    if (node.id === target) {
      path.add(node.id);
      return true;
    }
    if (node.children.some(visit)) {
      path.add(node.id);
      return true;
    }
    return false;
  };
  visit(root);
  return path;
}

function buildTopologyTree(
  data: TopologyData,
  sessions: Record<string, SessionLoad>,
): TopologyNode {
  const tenants = orderTenants(data.tenants).map((tenant) => tenantNode(tenant, sessions));
  return {
    id: "service",
    parentId: null,
    label: "aibox Service",
    detail: `${tenants.length} Tenants`,
    icon: "service",
    tone: maxTone(tenants.map((tenant) => tenant.tone)),
    children: tenants,
  };
}

function tenantNode(tenant: TopologyTenant, sessions: Record<string, SessionLoad>): TopologyNode {
  const id = tenantId(tenant);
  const scope = tenantScope(tenant);
  const agents = (["codex", "claude"] as const)
    .map((agent) => tenant.agents.find((entry) => entry.agent === agent))
    .filter((agent): agent is TopologyAgent => Boolean(agent))
    .map((agent) => agentNode(id, scope, agent, sessions));
  const components = componentNode(id, scope, tenant.components.entries, tenant.components.error);
  const children = [...agents, components];
  return {
    id,
    parentId: "service",
    label: tenant.display_name,
    detail: tenant.home,
    title: tenant.home,
    icon: tenant.kind === "host" ? "host" : "tenant",
    tone: tenant.exists ? maxTone(children.map((child) => child.tone)) : "warning",
    target: { module: "tenants", query: scopeLocation(scope) },
    children,
  };
}

function agentNode(
  tenantIdValue: string,
  scope: Scope,
  agent: TopologyAgent,
  sessions: Record<string, SessionLoad>,
): TopologyNode {
  const id = `${tenantIdValue}/agent:${agent.agent}`;
  const configParams = scopeLocation(scope);
  configParams.set("agent", agent.agent);
  configParams.set("current", "1");
  const currentTone: Tone = agent.current_config.error ? "error" : "neutral";
  const current: TopologyNode = {
    id: `${id}/current`,
    parentId: id,
    label: "Current Config",
    detail: agent.current_config.error
      ? "Inspection failed"
      : `${agent.current_config.present_files}/${agent.current_config.expected_files} files present`,
    title: agent.current_config.error,
    icon: "current",
    tone: currentTone,
    target: { module: "configs", query: configParams },
    children: [],
  };
  const namedId = `${id}/named-configs`;
  const namedChildren = agent.named_configs.entries.map((entry) => {
    const params = scopeLocation(scope);
    params.set("agent", agent.agent);
    params.set("config", entry.name);
    const last = agent.application.last_application?.applied === entry.name;
    return {
      id: `${namedId}/${entry.name}`,
      parentId: namedId,
      label: entry.name,
      detail: `${capitalize(entry.state)}${last ? ` · Last applied ${formatTimestamp(agent.application.last_application!.applied_at)}` : ""}`,
      title: entry.detail,
      icon: "config" as const,
      tone:
        entry.state === "invalid"
          ? ("error" as const)
          : entry.state === "incomplete"
            ? ("warning" as const)
            : ("good" as const),
      target: { module: "configs" as const, query: params },
      children: [],
    };
  });
  const namedParams = scopeLocation(scope);
  namedParams.set("agent", agent.agent);
  const named: TopologyNode = {
    id: namedId,
    parentId: id,
    label: "Named Configs",
    detail: agent.named_configs.error
      ? "Catalog inspection failed"
      : `${namedChildren.length} Configs`,
    title: agent.named_configs.error,
    icon: "configs",
    tone: agent.named_configs.error ? "error" : maxTone(namedChildren.map((node) => node.tone)),
    target: { module: "configs", query: namedParams },
    children: namedChildren,
  };
  const sessionId = `${id}/sessions`;
  const sessionLoad = sessions[sessionId];
  const sessionParams = new URLSearchParams();
  sessionParams.append("scope", scopeLocationValue(scope));
  sessionParams.append("agent", agent.agent);
  const sessionChildren: TopologyNode[] = sessionLoad
    ? [sessionSummaryNode(sessionId, sessionLoad)]
    : [];
  const sessionTone =
    sessionLoad?.state === "error" || sessionLoad?.data?.partial ? "warning" : "neutral";
  const sessionNode: TopologyNode = {
    id: sessionId,
    parentId: id,
    label: "Sessions",
    detail: sessionLoadDetail(sessionLoad),
    title: sessionLoad?.error ?? sessionLoad?.data?.warnings.join("\n"),
    icon: "sessions",
    tone: sessionTone,
    target: { module: "sessions", query: sessionParams },
    sessionRequest: { scope, agent: agent.agent },
    children: sessionChildren,
  };
  const children = [current, named, sessionNode];
  const driftTone = configDriftTone(agent.application.drift);
  const last = agent.application.last_application;
  const drift = humanDrift(agent.application.drift);
  return {
    id,
    parentId: tenantIdValue,
    label: agent.agent === "codex" ? "Codex" : "Claude",
    detail: last ? `Last applied ${last.applied} · ${drift}` : `Config Drift ${drift}`,
    title: agent.application.detail,
    icon: agent.agent,
    tone: maxTone([driftTone, ...children.map((child) => child.tone)]),
    target: { module: "configs", query: configParams },
    children,
  };
}

function componentNode(
  tenantIdValue: string,
  scope: Scope,
  entries: ComponentRow[],
  error?: string,
): TopologyNode {
  const id = `${tenantIdValue}/components`;
  const visible = entries.filter((entry) => entry.status !== "not-installed" || entry.error);
  const children = visible.map((entry) => {
    const params = scopeLocation(scope);
    params.set("component", entry.kind);
    return {
      id: `${id}/${entry.kind}`,
      parentId: id,
      label: componentLabel(entry.kind),
      detail: componentDetail(entry),
      title: entry.error ?? undefined,
      icon: "component" as const,
      tone: componentTone(entry),
      target: { module: "tenants" as const, query: params },
      children: [],
    };
  });
  const installed = entries.filter((entry) => entry.status === "installed").length;
  const params = scopeLocation(scope);
  return {
    id,
    parentId: tenantIdValue,
    label: "Components",
    detail: error ? "Catalog inspection failed" : `${installed}/${entries.length} installed`,
    title: error,
    icon: "components",
    tone: error ? "error" : maxTone(children.map((child) => child.tone)),
    target: { module: "tenants", query: params },
    children,
  };
}

function sessionSummaryNode(parentId: string, load: SessionLoad): TopologyNode {
  if (load.state === "loading") {
    return {
      id: `${parentId}/summary`,
      parentId,
      label: "Discovering Transcripts",
      icon: "session-summary",
      tone: "neutral",
      children: [],
    };
  }
  if (load.state === "error") {
    return {
      id: `${parentId}/summary`,
      parentId,
      label: "Session summary unavailable",
      detail: load.error,
      title: load.error,
      icon: "session-summary",
      tone: "error",
      children: [],
    };
  }
  const data = load.data!;
  return {
    id: `${parentId}/summary`,
    parentId,
    label: `${data.count} ${data.count === 1 ? "Session" : "Sessions"}`,
    detail: data.partial ? `${data.warnings.length} traversal warnings` : "Discovery complete",
    title: data.warnings.join("\n") || undefined,
    icon: "session-summary",
    tone: data.partial ? "warning" : "good",
    children: [],
  };
}

function summarizeTopology(data: TopologyData): TopologyHealth {
  const summary: TopologyHealth = {
    configTotal: 0,
    configAttention: 0,
    configErrors: 0,
    componentInstalled: 0,
    componentAttention: 0,
    componentErrors: 0,
  };
  for (const tenant of data.tenants) {
    for (const agent of tenant.agents) {
      summary.configTotal += agent.named_configs.entries.length;
      if (agent.current_config.error) {
        summary.configAttention += 1;
        summary.configErrors += 1;
      }
      if (["dirty", "source-missing", "comparison-error"].includes(agent.application.drift)) {
        summary.configAttention += 1;
        if (agent.application.drift === "comparison-error") summary.configErrors += 1;
      }
      if (agent.named_configs.error) {
        summary.configAttention += 1;
        summary.configErrors += 1;
      }
      for (const entry of agent.named_configs.entries) {
        if (entry.state === "incomplete" || entry.state === "invalid") summary.configAttention += 1;
        if (entry.state === "invalid") summary.configErrors += 1;
      }
    }
    if (tenant.components.error) {
      summary.componentAttention += 1;
      summary.componentErrors += 1;
    }
    for (const entry of tenant.components.entries) {
      if (entry.status === "installed") summary.componentInstalled += 1;
      if (entry.error || ["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) {
        summary.componentAttention += 1;
      }
      if (entry.error) summary.componentErrors += 1;
    }
  }
  return summary;
}

function orderTenants(tenants: TopologyTenant[]): TopologyTenant[] {
  const defaultTenant = tenants.find(
    (tenant) => tenant.kind === "managed" && tenant.name === "default",
  );
  const host = tenants.find((tenant) => tenant.kind === "host");
  const rest = tenants
    .filter((tenant) => tenant !== defaultTenant && tenant !== host)
    .sort((left, right) => left.display_name.localeCompare(right.display_name));
  return [host, defaultTenant, ...rest].filter((tenant): tenant is TopologyTenant =>
    Boolean(tenant),
  );
}

function tenantId(tenant: TopologyTenant): string {
  return tenant.kind === "host" ? "tenant:host" : `tenant:managed:${tenant.name}`;
}

function tenantScope(tenant: TopologyTenant): Scope {
  return tenant.kind === "host" ? { scope: "host" } : { scope: "managed", tenant: tenant.name! };
}

function scopeLocation(scope: Scope): URLSearchParams {
  return new URLSearchParams({ scope: scopeLocationValue(scope) });
}

function scopeLocationValue(scope: Scope): string {
  return scope.scope === "host" ? "host" : `managed:${scope.tenant}`;
}

function structuralIds(data: TopologyData): Set<string> {
  const ids = new Set<string>();
  for (const tenant of data.tenants) {
    const base = tenantId(tenant);
    ids.add(base);
    for (const agent of tenant.agents) {
      const agentId = `${base}/agent:${agent.agent}`;
      ids.add(agentId);
      if (agent.named_configs.entries.length) ids.add(`${agentId}/named-configs`);
    }
    if (
      tenant.components.entries.some((entry) => entry.status !== "not-installed" || entry.error)
    ) {
      ids.add(`${base}/components`);
    }
  }
  return ids;
}

function defaultExpansion(data: TopologyData): Set<string> {
  const primary =
    data.tenants.find((tenant) => tenant.kind === "managed" && tenant.name === "default") ??
    data.tenants.find((tenant) => tenant.kind === "host");
  if (!primary) return new Set();
  const base = tenantId(primary);
  const expanded = new Set([base]);
  expanded.add(`${base}/agent:codex`);
  expanded.add(`${base}/agent:codex/named-configs`);
  return expanded;
}

function collectVisibleNodes(
  root: TopologyNode,
  expanded: Set<string>,
  forced: Set<string>,
  parentId: string | null = null,
): Array<{ node: TopologyNode; parentId: string | null }> {
  const result = [{ node: root, parentId }];
  const open = root.id === "service" || expanded.has(root.id) || forced.has(root.id);
  if (open) {
    for (const child of root.children)
      result.push(...collectVisibleNodes(child, expanded, forced, root.id));
  }
  return result;
}

function collectBranchIds(node: TopologyNode, result: Set<string>) {
  if (node.children.length) result.add(node.id);
  for (const child of node.children) collectBranchIds(child, result);
}

function searchTopology(root: TopologyNode | null, query: string): TopologySearchResult {
  const matches = new Set<string>();
  const context = new Set<string>();
  const normalized = query.toLocaleLowerCase();
  let firstMatch: string | null = null;
  if (!root || !normalized) return { matches, context, firstMatch };
  const visit = (node: TopologyNode, ancestors: string[]) => {
    const text = `${node.label} ${node.detail ?? ""} ${node.title ?? ""}`.toLocaleLowerCase();
    if (text.includes(normalized)) {
      matches.add(node.id);
      firstMatch ??= node.id;
      for (const id of ancestors) context.add(id);
    }
    for (const child of node.children) visit(child, [...ancestors, node.id]);
  };
  visit(root, []);
  return { matches, context, firstMatch };
}

function filterByAttention(node: TopologyNode): TopologyNode | null {
  const children = node.children
    .map(filterByAttention)
    .filter((child): child is TopologyNode => Boolean(child));
  const matches = node.tone === "warning" || node.tone === "error";
  return matches || children.length ? { ...node, children } : null;
}

function emptyFilteredRoot(root: TopologyNode, detail: string): TopologyNode {
  return { ...root, detail, tone: "good", children: [] };
}

function targetHref(target: NavigationTarget): string {
  const query = target.query?.toString();
  return `/_aibox/ui/${target.module}${query ? `?${query}` : ""}`;
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

function maxTone(tones: Tone[]): Tone {
  if (tones.includes("error")) return "error";
  if (tones.includes("warning")) return "warning";
  if (tones.includes("good")) return "good";
  return "neutral";
}

function configDriftTone(drift: string): Tone {
  if (drift === "comparison-error") return "error";
  if (drift === "dirty" || drift === "source-missing") return "warning";
  if (drift === "clean") return "good";
  return "neutral";
}

function humanDrift(drift: string): string {
  return drift.split("-").map(capitalize).join(" ");
}

function componentTone(entry: ComponentRow): Tone {
  if (entry.error) return "error";
  if (["modified", "incomplete", "unmanaged"].includes(entry.status ?? "")) return "warning";
  if (entry.status === "installed") return "good";
  return "neutral";
}

function componentLabel(kind: string): string {
  return kind.split("-").map(capitalize).join(" ");
}

function componentDetail(entry: ComponentRow): string {
  if (entry.error) return "Inspection failed";
  const status = entry.status ? capitalize(entry.status) : "Unknown";
  return entry.version ? `${status} · ${entry.version}` : status;
}

function sessionLoadDetail(load?: SessionLoad): string {
  if (!load) return "Load count on demand";
  if (load.state === "loading") return "Discovering Transcripts";
  if (load.state === "error") return "Summary unavailable";
  return `${load.data!.count} Sessions${load.data!.partial ? " · Partial" : ""}`;
}

function attentionValue(value: number): string {
  return value === 0 ? "Healthy" : `${value} need attention`;
}

function healthTone(attention?: number, errors?: number, loadError?: string | null): Tone {
  if (loadError || errors) return "error";
  if (attention) return "warning";
  return attention === 0 ? "good" : "neutral";
}

function requestDetail(data: OverviewData): string {
  const states = [
    data.requests.active ? `${data.requests.active} active` : "",
    data.requests.warning ? `${data.requests.warning} warning` : "",
    data.requests.error ? `${data.requests.error} error` : "",
    formatBytes(data.requests.bytes),
  ].filter(Boolean);
  return states.join(" · ");
}

function imageTone(status?: OverviewData["runtime_image"]["status"]): Tone {
  if (status === "built") return "good";
  if (status === "missing") return "warning";
  return "neutral";
}

function shortImageId(id: string | null | undefined): string {
  if (!id) return "—";
  const value = id.startsWith("sha256:") ? id.slice(7) : id;
  return value.slice(0, 12);
}

function buildDisabledReason(data: OverviewData | null, operation: Operation | null): string {
  if (operation?.state === "running") return `Unavailable while ${operation.kind} is running`;
  if (data?.docker.status === "unavailable") return data.docker.error ?? "Docker is unavailable";
  return "Status is still loading";
}

function formatDuration(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const remainder = seconds % 60;
  if (days) return `${days}d ${hours}h`;
  if (hours) return `${hours}h ${minutes}m`;
  if (minutes) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}

function capitalize(value: string): string {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}
