import {
  AlertTriangle,
  Box,
  HardDrive,
  LoaderCircle,
  ChevronsDownUp,
  ChevronsUpDown,
  Minus,
  Network,
  Plus,
  Server,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import type { Operation } from "@/api/operations";
import type { OverviewApi } from "@/api/overview";
import { ErrorBanner, Fact, Metadata } from "@/features/overview/components/OverviewFacts";
import { RuntimeSection } from "@/features/overview/components/RuntimeSection";
import { TopologyCanvas } from "@/features/overview/topology/TopologyCanvas";
import {
  attentionCountLabel,
  attentionValue,
  collectVisibleNodes,
  findTopologyNode,
  formatDuration,
  healthTone,
  MAX_ZOOM,
  MIN_ZOOM,
} from "@/features/overview/topology/topologyModel";
import { useOverviewController } from "@/features/overview/useOverviewController";
import { moduleIcons, resourceIcons } from "@/shared/icons/consoleIcons";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SectionHeader } from "@/shared/ui/SurfacePrimitives";
import styles from "@/features/overview/OverviewPage.module.css";

const ComponentGroupIcon = resourceIcons.components;
const ConfigsModuleIcon = moduleIcons.configs;
const HostTenantIcon = resourceIcons.hostTenant;
const TenantsModuleIcon = moduleIcons.tenants;

interface OverviewPageProps {
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

export function OverviewPage(props: OverviewPageProps) {
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const { attention, service, topology: tree } = useOverviewController(props);
  const {
    build,
    buildDisabled,
    buildUnavailableReason,
    elapsedUptime,
    loadOverview,
    overview,
    overviewError,
    overviewRefreshing,
  } = service;
  const { attentionItems, health, panel } = attention;
  // The group is aliased because it holds a `topology` of its own: the tree data
  // sits inside the tree concern alongside the viewport state that renders it.
  const {
    collapseAll,
    expandAll,
    expanded,
    filteredTree,
    forcedExpanded,
    loadSessionSummary,
    loadTopology,
    metrics,
    navigateTree,
    pageRef,
    registerNode,
    renderedActiveNode,
    resetZoom,
    sessionLoads,
    setActiveNode,
    toggleNode,
    topology,
    topologyError,
    topologyRefreshing,
    treeRef,
    updateMetrics,
    zoom,
    zoomIn,
    zoomOut,
  } = tree;
  const { onNavigate, operation } = props;
  const selectedNode = useMemo(
    () => findTopologyNode(filteredTree, selectedNodeId ?? ""),
    [filteredTree, selectedNodeId],
  );
  useEffect(() => {
    if (!selectedNodeId || !filteredTree) return;
    const visibleIds = new Set(
      collectVisibleNodes(filteredTree, expanded, forcedExpanded).map(({ node }) => node.id),
    );
    if (visibleIds.has(selectedNodeId)) return;
    let ancestor = findTopologyNode(filteredTree, selectedNodeId)?.parentId ?? null;
    while (ancestor && !visibleIds.has(ancestor)) {
      ancestor = findTopologyNode(filteredTree, ancestor)?.parentId ?? null;
    }
    // Selection must follow the tree when a collapse or refresh hides its node.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSelectedNodeId(ancestor);
    setActiveNode(ancestor ?? filteredTree.id);
  }, [expanded, filteredTree, forcedExpanded, selectedNodeId, setActiveNode]);
  const selectNode = (id: string) => {
    if (selectedNodeId === id) {
      setSelectedNodeId(null);
      return;
    }
    setActiveNode(id);
    setSelectedNodeId(id);
    const node = findTopologyNode(filteredTree, id);
    if (node?.sessionRequest) void loadSessionSummary(id, node.sessionRequest);
  };

  return (
    <div ref={pageRef} className={styles.page} data-overview-scroll data-scroll-axis="vertical">
      {overviewError && <ErrorBanner message={overviewError} />}
      <section className={styles.summarySection} aria-label="Service status">
        <SectionHeader
          className={styles.sectionHeading}
          eyebrow="Operational status"
          title="Key facts"
          action={
            <RefreshButton
              label="Refresh status"
              busyLabel="Refreshing status"
              busy={overviewRefreshing}
              disabled={overviewRefreshing}
              onClick={() => void loadOverview(true)}
            >
              Refresh
            </RefreshButton>
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
          />
        </div>
        <section className={styles.attentionPanel} aria-labelledby="attention-title">
          <div>
            <span>Attention summary</span>
            <h3 id="attention-title">Needs attention</h3>
          </div>
          {panel === "pending" ? (
            <p className={styles.attentionPending} role="status">
              <LoaderCircle className="spin" size={15} aria-hidden="true" /> Inspecting service and
              topology
            </p>
          ) : panel === "healthy" ? (
            <p className={styles.healthySummary} role="status">
              No warnings or errors are currently reported.
            </p>
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
            label="AIBox Root"
            value={
              overview ? abbreviateTenantHome(overview.service.aibox_root, overview.host_home) : "—"
            }
            title={overview?.service.aibox_root ?? "—"}
            mono
            wide
          />
        </div>
      </section>

      <RuntimeSection
        overview={overview}
        operation={operation}
        buildDisabled={buildDisabled}
        buildUnavailableReason={buildUnavailableReason}
        onBuild={(force) => void build(force)}
      />

      <section ref={treeRef} className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyToolbar} data-overview-toolbar>
          <div className={styles.topologyTitleBlock}>
            <h2 id="topology-title">Resource topology</h2>
            <span>
              {topology
                ? `${topology.tenants.filter((tenant) => tenant.kind === "managed").length} Managed · ${topology.tenants.filter((tenant) => tenant.kind === "host").length} Host · ${health ? attentionCountLabel(health.configAttention + health.componentAttention) : "… need attention"}`
                : "Loading topology"}
            </span>
          </div>
          <div className={styles.topologyActions}>
            <button
              type="button"
              onClick={() => {
                collapseAll();
                setSelectedNodeId(null);
                setActiveNode("service");
              }}
              disabled={!topology}
            >
              <ChevronsDownUp size={15} aria-hidden="true" /> Collapse all
            </button>
            <button type="button" onClick={expandAll} disabled={!topology}>
              <ChevronsUpDown size={15} aria-hidden="true" /> Expand all
            </button>
            <div className={styles.zoomControls} aria-label="Topology zoom controls">
              <IconButton
                label="Zoom out"
                disabled={!metrics || zoom <= MIN_ZOOM}
                onClick={zoomOut}
              >
                <Minus size={15} />
              </IconButton>
              <button
                type="button"
                className={styles.zoomValue}
                disabled={!metrics}
                aria-label={`Reset topology zoom to 100% (currently ${Math.round(zoom * 100)}%)`}
                onClick={resetZoom}
              >
                {Math.round(zoom * 100)}%
              </button>
              <IconButton label="Zoom in" disabled={!metrics || zoom >= MAX_ZOOM} onClick={zoomIn}>
                <Plus size={15} />
              </IconButton>
            </div>
            <RefreshButton
              label="Refresh topology"
              busyLabel="Refreshing topology"
              busy={topologyRefreshing}
              disabled={topologyRefreshing}
              onClick={() => void loadTopology(true)}
            >
              Refresh
            </RefreshButton>
          </div>
        </div>
        <>
          {topologyError && (
            <ErrorBanner message={`Topology unavailable: ${topologyError}`} local />
          )}
          {!filteredTree && !topologyError && (
            <div className={styles.treeLoading}>
              <LoaderCircle className="spin" size={20} /> Inspecting Tenant state
            </div>
          )}
          {filteredTree && (
            <div className={styles.topologyWorkspace}>
              <TopologyCanvas
                root={filteredTree}
                expanded={expanded}
                forcedExpanded={forcedExpanded}
                activeNode={renderedActiveNode}
                selectedNode={selectedNode}
                zoom={zoom}
                sessionLoads={sessionLoads}
                onMetricsChange={updateMetrics}
                registerNode={registerNode}
                onFocus={setActiveNode}
                onSelect={selectNode}
                onCloseInspector={() => setSelectedNodeId(null)}
                onNavigate={onNavigate}
                onKeyDown={(event, node) => {
                  if (event.key === "Enter") selectNode(node.id);
                  navigateTree(event, node);
                }}
                onToggle={toggleNode}
                onRefreshSession={(node) =>
                  node.sessionRequest && void loadSessionSummary(node.id, node.sessionRequest, true)
                }
              />
            </div>
          )}
        </>
      </section>
    </div>
  );
}
