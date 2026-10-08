import { Container, Layers, Server } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";

import { RuntimeStatus } from "@/features/overview/components/OverviewFacts";
import { dockerTone, imageTone } from "@/features/overview/components/statusTone";
import { RuntimeSection } from "@/features/overview/components/RuntimeSection";
import { EnvironmentDetails } from "@/features/overview/components/EnvironmentDetails";
import { AttentionPanel } from "@/features/overview/components/AttentionPanel";
import { TenantStatusList } from "@/features/overview/components/TenantStatusList";
import { OverviewKpis } from "@/features/overview/components/OverviewKpis";
import { useOverviewController } from "@/features/overview/useOverviewController";

import { capitalize } from "@/shared/lib/format";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/overview/OverviewPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";
import type { OverviewPageProps } from "@/features/overview/viewTypes";

export function OverviewPage(props: OverviewPageProps) {
  const [initialBrowsing] = useState(() => props.browsingMemory?.current ?? null);
  const restorePending = useRef(initialBrowsing !== null);
  const { attention, service, topology: resources } = useOverviewController(props);
  const {
    overview,
    overviewError,
    overviewRefreshing,
    loadOverview,
    build,
    buildDisabled,
    buildUnavailableReason,
    elapsedUptime,
    requestsTotal,
  } = service;
  const { tenants, pageRef, topology, topologyError, topologyRefreshing, loadTopology } = resources;
  const hostCount = topology?.tenants.filter((tenant) => tenant.kind === "host").length ?? 0;
  const managedCount = topology?.tenants.filter((tenant) => tenant.kind === "managed").length ?? 0;
  const tenantSummary = topology
    ? hostCount > 0
      ? `${hostCount} Host · ${managedCount} Managed`
      : `${managedCount} Managed Tenants`
    : topologyError
      ? "Resource inspection unavailable"
      : "Loading resources";

  useLayoutEffect(() => {
    if (!restorePending.current || !initialBrowsing || !tenants || (!overview && !overviewError))
      return;
    if (pageRef.current) pageRef.current.scrollTop = initialBrowsing.scrollTop;
    restorePending.current = false;
  }, [initialBrowsing, tenants, overview, overviewError, pageRef]);
  useLayoutEffect(() => {
    const memory = props.browsingMemory;
    const page = pageRef.current;
    if (!memory || !page || !tenants) return;
    return () => {
      if (!restorePending.current) memory.current = { scrollTop: page.scrollTop };
    };
  }, [tenants, pageRef, props.browsingMemory]);

  return (
    <div ref={pageRef} className={styles.page} data-overview-scroll data-scroll-axis="vertical">
      <div className={styles.pageHeader}>
        <div className={styles.headerLeft}>
          <h1 className={styles.pageTitle}>Overview</h1>
          <p className={styles.pageSubtitle}>System health, metrics, and resource topology</p>
        </div>
        <div className={styles.headerActions}>
          <RefreshButton
            label="Refresh Overview"
            compactOnNarrow
            busyLabel="Refreshing Overview"
            busy={overviewRefreshing || topologyRefreshing}
            disabled={overviewRefreshing || topologyRefreshing}
            onClick={() => void Promise.all([loadOverview(true), loadTopology(true)])}
          >
            Refresh
          </RefreshButton>
        </div>
      </div>

      <section className={styles.statusSection} aria-label="Service status">
        <div className={styles.systemStatusBar}>
          <div className={styles.systemStatusMain}>
            <span className={styles.systemStatusTitle}>System health</span>
            <div className={styles.statusMain}>
              <RuntimeStatus
                icon={<Server size={iconSize.xs} />}
                label="Service"
                value={overviewError ? "Unavailable" : overview ? "Running" : "Loading"}
                tone={overviewError ? "error" : overview ? "good" : "neutral"}
              />
              <RuntimeStatus
                icon={<Container size={iconSize.xs} />}
                label="Docker"
                value={
                  overviewError ? "Unknown" : capitalize(overview?.docker.status ?? "checking")
                }
                tone={overviewError ? "neutral" : dockerTone(overview?.docker.status)}
              />
              <div
                className={styles.runtimeGroup}
                role="group"
                aria-label="Runtime Image management"
              >
                <RuntimeStatus
                  icon={<Layers size={iconSize.xs} />}
                  label="Runtime Image"
                  value={
                    overviewError
                      ? "Unknown"
                      : capitalize(overview?.runtime_image.status ?? "checking")
                  }
                  tone={overviewError ? "neutral" : imageTone(overview?.runtime_image.status)}
                />
                <RuntimeSection
                  overview={overview}
                  operation={props.operation}
                  buildDisabled={buildDisabled}
                  buildUnavailableReason={buildUnavailableReason}
                  onBuild={(force) => void build(force)}
                />
              </div>
            </div>
          </div>

          <div className={styles.systemStatusActions}>
            <EnvironmentDetails overview={overview} elapsedUptime={elapsedUptime} />
          </div>

          {buildUnavailableReason && (
            <span id="runtime-build-unavailable" className="srOnly">
              {buildUnavailableReason}
            </span>
          )}
        </div>
      </section>

      <OverviewKpis
        overview={overview}
        topology={topology}
        requestsTotal={requestsTotal}
        onNavigate={props.onNavigate}
      />

      <AttentionPanel
        panel={attention.panel}
        items={attention.attentionItems}
        onNavigate={props.onNavigate}
        onRetry={(source) => void (source === "service" ? loadOverview(true) : loadTopology(true))}
        serviceRetrying={overviewRefreshing}
        topologyRetrying={topologyRefreshing}
      />

      <section className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyHeading}>
          <div>
            <h2 id="topology-title">Tenants</h2>
            <p>{tenantSummary}</p>
          </div>
        </div>
        {!tenants && !topologyError && (
          <p className={styles.catalogMessage} role="status">
            Inspecting Tenant state
          </p>
        )}
        {tenants &&
          (tenants.length ? (
            <TenantStatusList tenants={tenants} onNavigate={props.onNavigate} />
          ) : (
            <p className={styles.catalogMessage}>No Tenants are currently reported.</p>
          ))}
      </section>
    </div>
  );
}
