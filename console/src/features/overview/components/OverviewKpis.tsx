import { ArrowUpRight } from "lucide-react";
import type { OverviewData, TopologyData } from "@/api/overview";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { iconSize } from "@/shared/icons/iconSizes";
import { moduleIcons } from "@/shared/icons/consoleIcons";
import styles from "@/features/overview/components/OverviewKpis.module.css";

interface OverviewKpisProps {
  overview: OverviewData | null;
  topology: TopologyData | null;
  requestsTotal?: number | null;
  onNavigate: ConsoleNavigate;
}

export function OverviewKpis({ overview, topology, requestsTotal, onNavigate }: OverviewKpisProps) {
  const managedCount = topology
    ? topology.tenants.filter((t) => t.kind === "managed").length
    : (overview?.managed_tenants ?? 0);
  const hostCount = topology
    ? topology.tenants.filter((t) => t.kind === "host").length
    : overview?.host_available
      ? 1
      : 0;

  const totalNamedConfigs = topology
    ? topology.tenants.reduce(
        (acc, t) => acc + t.agents.reduce((a, ag) => a + (ag.named_configs?.count ?? 0), 0),
        0,
      )
    : 0;

  const totalCurrentConfigs = topology
    ? topology.tenants.reduce(
        (acc, t) => acc + t.agents.filter((ag) => ag.current_config.present_files > 0).length,
        0,
      )
    : 0;

  const totalSessions = topology
    ? topology.tenants.reduce(
        (acc, t) => acc + t.agents.reduce((a, ag) => a + (ag.sessions?.count ?? 0), 0),
        0,
      )
    : 0;

  const componentsInstalled = topology
    ? topology.tenants.reduce((acc, t) => acc + (t.components?.installed ?? 0), 0)
    : 0;
  const componentsTotal = topology
    ? topology.tenants.reduce((acc, t) => acc + (t.components?.total ?? 0), 0)
    : 0;

  const hasRequestsCount = requestsTotal !== null && requestsTotal !== undefined;

  const totalTenants = hostCount + managedCount;

  const kpis = [
    {
      id: "tenants",
      label: "Tenants",
      value: totalTenants,
      unit: "Total Tenants",
      subtext: `${hostCount > 0 ? `${hostCount} Host · ${managedCount} Managed` : `${managedCount} Managed`} · ${componentsInstalled}/${componentsTotal} Components`,
      icon: moduleIcons.tenants,
      onClick: () => onNavigate("tenants"),
      ariaLabel: `View ${totalTenants} Tenants (${hostCount} Host, ${managedCount} Managed) and ${componentsInstalled} of ${componentsTotal} Installed Components`,
    },
    {
      id: "configs",
      label: "Configs",
      value: totalCurrentConfigs,
      unit: "Current Configs",
      subtext: `${totalNamedConfigs} Named Profiles across Tenants`,
      icon: moduleIcons.configs,
      onClick: () => onNavigate("configs"),
      ariaLabel: `View ${totalCurrentConfigs} Current Configs and ${totalNamedConfigs} Named Configs`,
    },
    {
      id: "sessions",
      label: "Sessions",
      value: totalSessions,
      unit: "Total Sessions",
      subtext: "Codex & Claude Histories",
      icon: moduleIcons.sessions,
      onClick: () => onNavigate("sessions"),
      ariaLabel: `View ${totalSessions} Agent Sessions`,
    },
    {
      id: "requests",
      label: "Requests",
      value: hasRequestsCount ? requestsTotal.toLocaleString() : "Audit Log",
      unit: hasRequestsCount ? "Total Requests" : "HTTP & SSE",
      subtext: "HTTP & SSE · Audit & Trace",
      icon: moduleIcons.requests,
      onClick: () => onNavigate("requests"),
      ariaLabel: hasRequestsCount
        ? `View ${requestsTotal} Requests in Audit Log`
        : "View Requests Audit Log and Proxy Traffic",
    },
  ];

  return (
    <section className={styles.kpiGrid} aria-label="System Metrics Overview">
      {kpis.map((kpi) => {
        const Icon = kpi.icon;
        return (
          <button
            key={kpi.id}
            type="button"
            className={styles.kpiCard}
            onClick={kpi.onClick}
            aria-label={kpi.ariaLabel}
          >
            <div className={styles.kpiHeader}>
              <span className={styles.kpiLabel}>{kpi.label}</span>
              <span className={styles.kpiIcon}>
                <Icon size={iconSize.sm} aria-hidden="true" />
              </span>
            </div>
            <div className={styles.kpiValueRow}>
              <span className={styles.kpiValue}>{kpi.value}</span>
              <span className={styles.kpiUnit}>{kpi.unit}</span>
              <span className={styles.jumpArrow} aria-hidden="true">
                <ArrowUpRight size={iconSize.xs} />
              </span>
            </div>
            <div className={styles.kpiSubtext}>{kpi.subtext}</div>
          </button>
        );
      })}
    </section>
  );
}
