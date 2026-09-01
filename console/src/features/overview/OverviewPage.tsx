import {
  AlertTriangle,
  Box,
  ChevronsDownUp,
  ChevronsUpDown,
  HardDrive,
  LoaderCircle,
  Minus,
  Network,
  Plus,
  Scan,
  Search,
  Server,
  ShieldAlert,
} from "lucide-react";

import type { Operation } from "@/api/operations";
import type { OverviewApi } from "@/api/overview";
import { ErrorBanner, Fact, Metadata } from "@/features/overview/components/OverviewFacts";
import { RuntimeSection } from "@/features/overview/components/RuntimeSection";
import { TopologyCanvas } from "@/features/overview/topology/TopologyCanvas";
import {
  attentionValue,
  formatDuration,
  healthTone,
  MAX_ZOOM,
  MIN_ZOOM,
  requestDetail,
} from "@/features/overview/topology/topologyModel";
import { useOverviewController } from "@/features/overview/useOverviewController";
import { moduleIcons, resourceIcons } from "@/shared/icons/consoleIcons";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { TextInput } from "@/shared/ui/FormControls";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SectionHeader } from "@/shared/ui/SurfacePrimitives";
import styles from "@/features/overview/OverviewPage.module.css";

const ComponentGroupIcon = resourceIcons.components;
const ConfigsModuleIcon = moduleIcons.configs;
const HostTenantIcon = resourceIcons.hostTenant;
const RequestsModuleIcon = moduleIcons.requests;
const TenantsModuleIcon = moduleIcons.tenants;

interface OverviewPageProps {
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

export function OverviewPage(props: OverviewPageProps) {
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
  const { attentionItems, attentionOnly, health, revealAttention, toggleAttention } = attention;
  // The group is aliased because it holds a `topology` of its own: the tree data
  // sits inside the tree concern alongside the viewport state that renders it.
  const {
    collapseAll,
    expandAll,
    expanded,
    filteredTree,
    fitTopology,
    forcedExpanded,
    loadSessionSummary,
    loadTopology,
    metrics,
    navigateTree,
    pageRef,
    query,
    registerNode,
    renderedActiveNode,
    resetZoom,
    sessionLoads,
    setActiveNode,
    setQuery,
    toggleNode,
    topology,
    topologyError,
    topologyRefreshing,
    topologySearch,
    treeRef,
    updateMetrics,
    zoom,
    zoomIn,
    zoomMode,
    zoomOut,
  } = tree;
  const { onNavigate, operation } = props;

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
            label="AIBox Root"
            value={overview?.service.aibox_root ?? "—"}
            mono
            wide
          />
        </div>
      </section>

      <section ref={treeRef} className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyHeading}>
          <SectionHeader
            className={styles.sectionHeading}
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
          <div className={styles.topologyToolbar} data-overview-toolbar>
            <label className={styles.searchField}>
              <Search size={15} aria-hidden="true" />
              <span className="srOnly">Filter topology</span>
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
              onClick={toggleAttention}
            >
              <ShieldAlert size={15} /> Needs attention
            </button>
            <IconButton
              label="Expand topology (Session summaries remain on demand)"
              disabled={!topology}
              onClick={expandAll}
            >
              <ChevronsUpDown size={16} />
            </IconButton>
            <IconButton
              label="Collapse all Tenant branches"
              disabled={!topology}
              onClick={collapseAll}
            >
              <ChevronsDownUp size={16} />
            </IconButton>
            <RefreshButton
              label="Refresh topology"
              busyLabel="Refreshing topology"
              busy={topologyRefreshing}
              iconOnly
              iconSize={16}
              disabled={topologyRefreshing}
              onClick={() => void loadTopology(true)}
            />
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
              <IconButton
                label="Fit topology to width"
                disabled={!metrics}
                aria-pressed={zoomMode === "fit"}
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
              zoom={zoom}
              sessionLoads={sessionLoads}
              onMetricsChange={updateMetrics}
              registerNode={registerNode}
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

      <RuntimeSection
        overview={overview}
        operation={operation}
        buildDisabled={buildDisabled}
        buildUnavailableReason={buildUnavailableReason}
        onBuild={(force) => void build(force)}
      />
    </div>
  );
}
