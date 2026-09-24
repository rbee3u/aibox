import { Boxes } from "lucide-react";
import type { ReactNode } from "react";
import { OverviewLink } from "@/features/overview/OverviewLink";
import type { TopologyNode } from "@/features/overview/resourceTree";
import type { Tone } from "@/features/overview/viewTypes";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import styles from "@/features/overview/components/TenantStatusList.module.css";
import { resourceIcons, toneIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

/**
 * The mark for a tone that needs work, or nothing when it does not.
 */
function ToneMark({ tone }: { tone: Tone }) {
  if (tone !== "warning" && tone !== "error") return null;
  const Icon = toneIcons[tone];
  return <Icon size={iconSize.xs} aria-hidden="true" />;
}

function ResourceLink({
  node,
  children,
  onNavigate,
  ariaLabel,
}: {
  node: TopologyNode;
  children: ReactNode;
  onNavigate: ConsoleNavigate;
  ariaLabel?: string;
}) {
  if (!node.target) return <span>{children}</span>;
  return (
    <OverviewLink
      className={styles.link}
      targetModule={node.target.module}
      query={node.target.query}
      onNavigate={onNavigate}
      aria-label={ariaLabel}
    >
      {children}
    </OverviewLink>
  );
}

/**
 * A status fact stated in place.
 */
function Issue({ node }: { node: TopologyNode }) {
  return (
    <span
      className={`${styles.issueBanner} ${
        node.tone === "error" ? styles.issueError : styles.issueWarning
      }`}
    >
      <ToneMark tone={node.tone} />
      <span>
        {node.label}: {node.detail}
      </span>
    </span>
  );
}

function AgentSummary({
  agent,
  node,
  onNavigate,
}: {
  agent: "codex" | "claude";
  node?: TopologyNode;
  onNavigate: ConsoleNavigate;
}) {
  if (!node) {
    return (
      <div className={styles.sectionContainer}>
        <div className={styles.sectionHeader}>
          <span className={styles.headerTitleGroup}>
            <BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />
            <span className={styles.agentName}>{agent === "codex" ? "Codex" : "Claude"}</span>
          </span>
        </div>
        <span className={styles.emptyStatusText}>Not reported</span>
      </div>
    );
  }

  const current = node.children.find((child) => child.icon === "current")!;
  const configs = node.children.find((child) => child.icon === "configs")!;
  const sessions = node.children.find((child) => child.icon === "sessions")!;

  return (
    <div className={styles.sectionContainer}>
      <div className={styles.sectionHeader}>
        <span className={styles.headerTitleGroup}>
          <BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />
          <span className={styles.agentName}>{agent === "codex" ? "Codex" : "Claude"}</span>
        </span>
      </div>

      <div className={styles.agentConfigRow}>
        <ResourceLink
          node={current}
          onNavigate={onNavigate}
          ariaLabel={node.detail ?? "Current Config"}
        >
          <span className={styles.currentConfigLink}>
            <span className={styles.currentConfigLabel}>Current Config</span>
            <span className={styles.currentConfigValue}>
              {node.detail ?? "No recorded application"}
            </span>
          </span>
        </ResourceLink>
      </div>

      <div className={styles.agentChipsRow}>
        <ResourceLink node={configs} onNavigate={onNavigate}>
          <span
            className={`${styles.actionPill} ${
              configs.tone === "error"
                ? styles.pillError
                : configs.tone === "warning"
                  ? styles.pillWarning
                  : ""
            }`}
          >
            <ToneMark tone={configs.tone} />
            <span>{configs.detail}</span>
          </span>
        </ResourceLink>
        <ResourceLink node={sessions} onNavigate={onNavigate}>
          <span
            className={`${styles.actionPill} ${sessions.tone === "error" ? styles.pillError : ""}`}
          >
            <ToneMark tone={sessions.tone} />
            <span>{sessions.detail}</span>
          </span>
        </ResourceLink>
      </div>

      {current.tone !== "neutral" && (
        <div className={styles.issueRow}>
          <Issue node={current} />
        </div>
      )}
    </div>
  );
}

export function TenantStatusList({
  root,
  onNavigate,
}: {
  root: TopologyNode;
  onNavigate: ConsoleNavigate;
}) {
  return (
    <table className={styles.table} aria-label="Tenant status">
      <thead>
        <tr>
          <th scope="col">Tenant</th>
          <th scope="col">Components</th>
          <th scope="col">Codex</th>
          <th scope="col">Claude</th>
        </tr>
      </thead>
      <tbody>
        {root.children.map((tenant) => {
          const components = tenant.children.find((child) => child.icon === "components")!;
          const Icon = tenant.icon === "host" ? HostTenantIcon : ManagedTenantIcon;
          const isHost = tenant.icon === "host";
          return (
            <tr key={tenant.id} className={styles.tenantCard}>
              <th scope="row" className={styles.tenantHeaderCell}>
                <div className={styles.tenantIdentity}>
                  <ResourceLink node={tenant} onNavigate={onNavigate}>
                    <span
                      className={`${styles.tenantIconWrapper} ${
                        isHost ? styles.hostIconWrapper : ""
                      }`}
                    >
                      <Icon size={iconSize.md} aria-hidden="true" />
                    </span>
                    <strong>{tenant.label}</strong>
                  </ResourceLink>
                  <span className={styles.secondary}>
                    {isHost ? "Host · Management only" : "Managed Tenant"}
                  </span>
                  {tenant.detail && <Issue node={tenant} />}
                </div>
              </th>
              <td className={styles.componentsCell}>
                <span className={styles.mobileLabel} aria-hidden="true">
                  Components
                </span>
                <div className={styles.sectionContainer}>
                  <div className={styles.sectionHeader}>
                    <span className={styles.headerTitleGroup}>
                      <Boxes size={iconSize.xs} className={styles.headerIcon} aria-hidden="true" />
                      <span className={styles.sectionTitle}>Components</span>
                    </span>
                  </div>
                  <div className={styles.componentsFactRow}>
                    <span
                      className={`${styles.fact} ${
                        components.tone === "error"
                          ? styles.error
                          : components.tone === "warning"
                            ? styles.warning
                            : ""
                      }`}
                    >
                      <ToneMark tone={components.tone} />
                      <span className={styles.componentDetailText}>{components.detail}</span>
                    </span>
                  </div>
                  <div className={styles.componentsActionRow}>
                    <ResourceLink node={components} onNavigate={onNavigate}>
                      <span className={styles.primaryLink}>
                        Manage components <span className={styles.arrowIcon}>→</span>
                      </span>
                    </ResourceLink>
                  </div>
                </div>
              </td>
              {(["codex", "claude"] as const).map((agent) => (
                <td key={agent} className={styles.agentCell}>
                  <span className={styles.mobileLabel} aria-hidden="true">
                    {agent === "codex" ? "Codex" : "Claude"}
                  </span>
                  <AgentSummary
                    agent={agent}
                    node={tenant.children.find((child) => child.icon === agent)}
                    onNavigate={onNavigate}
                  />
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
