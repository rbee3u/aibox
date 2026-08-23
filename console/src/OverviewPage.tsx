import {
  AlertTriangle,
  Box,
  ChevronsDownUp,
  ChevronsUpDown,
  Hammer,
  HardDrive,
  Image,
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
  formatBytes,
  type Operation,
  type OverviewData,
  type OverviewApi,
  type TopologyData,
} from "./controlApi";
import { IconButton } from "./components/IconButton";
import { ActionButton } from "./components/ActionButton";
import { TextInput } from "./components/FormControls";
import { moduleIcons, resourceIcons, type ModuleId } from "./consoleIcons";
import { formatTimestamp } from "./utils";
import { TopologyCanvas } from "./TopologyCanvas";
import {
  attentionValue,
  buildDisabledReason,
  buildTopologyTree,
  capitalize,
  clampZoom,
  collectBranchIds,
  collectVisibleNodes,
  defaultExpansion,
  emptyFilteredRoot,
  filterByAttention,
  firstComponentAttentionTarget,
  firstConfigAttentionTarget,
  fitTopologyZoom,
  formatDuration,
  healthTone,
  imageTone,
  MAX_ZOOM,
  MIN_ZOOM,
  requestAttentionDetail,
  requestDetail,
  searchTopology,
  shortImageId,
  structuralIds,
  summarizeTopology,
  type AttentionItem,
  type SessionLoad,
  type SessionRequest,
  type Tone,
  type TopologyMetrics,
  type TopologyNode,
} from "./overviewTopology";
import styles from "./OverviewPage.module.css";
const OVERVIEW_POLL_MS = 15000;
const ZOOM_STEP = 0.1;
const MOBILE_CANVAS_WIDTH = 760;
const ComponentGroupIcon = resourceIcons.components;
const ConfigsModuleIcon = moduleIcons.configs;
const HostTenantIcon = resourceIcons.hostTenant;
const RequestsModuleIcon = moduleIcons.requests;
const TenantsModuleIcon = moduleIcons.tenants;
export type ConsoleNavigate = (module: ModuleId, query?: URLSearchParams) => void;
interface OverviewPageProps {
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
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
        const value = await api.loadOverview(controller.signal);
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
        const value = await api.loadTopology(controller.signal);
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
    const tick = window.setInterval(() => setUptimeTick(Date.now()), 1000);
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
  const elapsedUptime = overview
    ? overview.service.uptime_seconds +
      Math.max(0, Math.floor((uptimeTick - overviewLoadedAt) / 1000))
    : 0;
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
        label: "Request Records",
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
              <RefreshCw className={overviewRefreshing ? "spin" : undefined} size={16} />
            </IconButton>
          }
        />
        <div className={styles.factGrid}>
          <Fact
            icon={<Server size={18} />}
            label="Service"
            value={overviewError ? "Unavailable" : overview ? "Running" : "Loading"}
            detail={overviewError ?? (overview ? formatDuration(elapsedUptime) : "Connecting")}
            tone={overviewError ? "error" : overview ? "good" : "neutral"}
          />
          <Fact
            icon={<TenantsModuleIcon size={18} />}
            label="Managed Tenants"
            value={overview?.managed_tenants ?? "—"}
            detail={overview ? "Runnable persistent identities" : "Loading"}
            tone="neutral"
            onClick={() => onNavigate("tenants")}
          />
          <Fact
            icon={<HostTenantIcon size={18} />}
            label="Host Tenant"
            value={overview ? (overview.host_available ? "Available" : "Unavailable") : "—"}
            detail="Console-only view of the Host Home"
            tone={overview?.host_available === false ? "warning" : "neutral"}
            onClick={() => onNavigate("tenants", new URLSearchParams("tenant=host"))}
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
        <section className={styles.attentionPanel} aria-labelledby="attention-title">
          <div>
            <span>Attention summary</span>
            <h3 id="attention-title">Needs attention</h3>
          </div>
          {attentionItems.length === 0 ? (
            <p className={styles.healthySummary}>No warnings or errors are currently reported.</p>
          ) : (
            <div className={styles.attentionList}>
              {attentionItems.map((item) => (
                <button
                  type="button"
                  key={`${item.label}:${item.detail}`}
                  className={styles[item.tone]}
                  disabled={!item.target}
                  onClick={() => item.target && onNavigate(item.target.module, item.target.query)}
                >
                  <AlertTriangle size={15} aria-hidden="true" />
                  <span>
                    <strong>{item.label}</strong>
                    <small>{item.detail}</small>
                  </span>
                </button>
              ))}
            </div>
          )}
        </section>
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

      <section ref={treeRef} className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyHeading}>
          <SectionHeading
            eyebrow="Persistent identities"
            title="Resource topology"
            id="topology-title"
          />
          <span>
            {topology
              ? `${topology.tenants.filter((tenant) => tenant.kind === "managed").length} Managed · ${topology.tenants.some((tenant) => tenant.kind === "host") ? "Host Tenant" : "No Host Tenant"}`
              : "Loading topology"}
          </span>
        </div>
        <>
          <div className={styles.topologyToolbar}>
            <label className={styles.searchField}>
              <Search size={15} aria-hidden="true" />
              <span className={styles.srOnly}>Filter topology</span>
              <TextInput
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
              <RefreshCw className={topologyRefreshing ? "spin" : undefined} size={16} />
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
          {topologyError && (
            <ErrorBanner message={`Topology unavailable: ${topologyError}`} local />
          )}
          {!filteredTree && !topologyError && (
            <div className={styles.treeLoading}>
              <LoaderCircle className="spin" size={20} /> Inspecting Tenant state
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
        </>
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
                <LoaderCircle className="spin" size={14} /> {operation.kind}
              </span>
            )}
            <ActionButton
              tone="primary"
              disabled={buildDisabled}
              aria-describedby={buildUnavailableReason ? "runtime-build-unavailable" : undefined}
              title={buildUnavailableReason ?? "Build Runtime Image using Docker cache"}
              onClick={() => void build(false)}
            >
              <Hammer size={15} /> Build
            </ActionButton>
            <ActionButton
              disabled={buildDisabled}
              aria-describedby={buildUnavailableReason ? "runtime-build-unavailable" : undefined}
              title={
                buildUnavailableReason ??
                "Re-run every layer without cache and pull a fresh base image"
              }
              onClick={() => void build(true)}
            >
              <RefreshCw size={15} /> Build without cache
            </ActionButton>
          </div>
        </div>
        {buildUnavailableReason && (
          <p id="runtime-build-unavailable" className={styles.buildUnavailable} role="status">
            {buildUnavailableReason}
          </p>
        )}
        {overview?.runtime_image.detail && (
          <div className={styles.runtimeNotice} role="status">
            <AlertTriangle size={15} /> {overview.runtime_image.detail}
          </div>
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
function scrollTopologyElement(
  element: Element | null | undefined,
  options: ScrollIntoViewOptions,
) {
  if (element && typeof element.scrollIntoView === "function") element.scrollIntoView(options);
}
