import { Image, Server } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { OverviewBrowsingMemory } from "@/features/overview/browsingState";
import type { Operation } from "@/api/operations";
import type { OverviewApi } from "@/api/overview";
import { RuntimeStatus } from "@/features/overview/components/OverviewFacts";
import { dockerTone, imageTone } from "@/features/overview/components/statusTone";
import { RuntimeSection } from "@/features/overview/components/RuntimeSection";
import { EnvironmentDetails } from "@/features/overview/components/EnvironmentDetails";
import { AttentionPanel } from "@/features/overview/components/AttentionPanel";
import { TenantStatusList } from "@/features/overview/components/TenantStatusList";
import { useOverviewController } from "@/features/overview/useOverviewController";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { capitalize } from "@/shared/lib/format";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/overview/OverviewPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

interface OverviewPageProps {
  browsingMemory?: OverviewBrowsingMemory;
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}
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
  } = service;
  const { tree, pageRef, topology, topologyError, topologyRefreshing, loadTopology } = resources;

  useLayoutEffect(() => {
    if (!restorePending.current || !initialBrowsing || !tree || (!overview && !overviewError))
      return;
    if (pageRef.current) pageRef.current.scrollTop = initialBrowsing.scrollTop;
    restorePending.current = false;
  }, [initialBrowsing, tree, overview, overviewError, pageRef]);
  useLayoutEffect(() => {
    const memory = props.browsingMemory;
    const page = pageRef.current;
    if (!memory || !page || !tree) return;
    return () => {
      if (!restorePending.current) memory.current = { scrollTop: page.scrollTop };
    };
  }, [tree, pageRef, props.browsingMemory]);

  return (
    <div ref={pageRef} className={styles.page} data-overview-scroll data-scroll-axis="vertical">
      <section className={styles.statusSection} aria-label="Service status">
        <div className={styles.statusStrip}>
          <div className={styles.statusMain}>
            <RuntimeStatus
              icon={<Server size={iconSize.xs} />}
              label="Service"
              value={overviewError ? "Unavailable" : overview ? "Running" : "Loading"}
              tone={overviewError ? "error" : overview ? "good" : "neutral"}
            />
            <RuntimeStatus
              icon={<Server size={iconSize.xs} />}
              label="Docker"
              value={overviewError ? "Unknown" : capitalize(overview?.docker.status ?? "checking")}
              tone={overviewError ? "neutral" : dockerTone(overview?.docker.status)}
            />
            <div className={styles.runtimeGroup} role="group" aria-label="Runtime Image management">
              <RuntimeStatus
                icon={<Image size={iconSize.xs} />}
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
            <div className={styles.statusActions}>
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
          {buildUnavailableReason && (
            <span id="runtime-build-unavailable" className="srOnly">
              {buildUnavailableReason}
            </span>
          )}
          <EnvironmentDetails overview={overview} elapsedUptime={elapsedUptime} />
        </div>
        <AttentionPanel
          panel={attention.panel}
          items={attention.attentionItems}
          onNavigate={props.onNavigate}
          onRetry={(source) =>
            void (source === "service" ? loadOverview(true) : loadTopology(true))
          }
          serviceRetrying={overviewRefreshing}
          topologyRetrying={topologyRefreshing}
        />
      </section>
      <section className={styles.topologySection} aria-labelledby="topology-title">
        <div className={styles.topologyHeading}>
          <div>
            <h2 id="topology-title">Tenants</h2>
            <p>
              {topology
                ? `${topology.tenants.filter((tenant) => tenant.kind === "managed").length} Managed · ${topology.tenants.filter((tenant) => tenant.kind === "host").length} Host`
                : topologyError
                  ? "Resource inspection unavailable"
                  : "Loading resources"}
            </p>
          </div>
        </div>
        {!tree && !topologyError && (
          <p className={styles.treeLoading} role="status">
            Inspecting Tenant state
          </p>
        )}
        {tree &&
          (tree.children.length ? (
            <TenantStatusList root={tree} onNavigate={props.onNavigate} />
          ) : (
            <p className={styles.treeLoading}>No Tenants are currently reported.</p>
          ))}
      </section>
    </div>
  );
}
