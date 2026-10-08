import { Boxes } from "lucide-react";
import type { ReactNode } from "react";
import { OverviewLink } from "@/features/overview/OverviewLink";
import type {
  AgentStatus,
  ResourceStatus,
  TenantStatusRow,
  Tone,
} from "@/features/overview/viewTypes";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import styles from "@/features/overview/components/TenantStatusList.module.css";
import { resourceIcons, toneIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { agentLabel } from "@/shared/lib/format";

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
  status,
  children,
  onNavigate,
  ariaLabel,
}: {
  status: ResourceStatus;
  children: ReactNode;
  onNavigate: ConsoleNavigate;
  ariaLabel?: string;
}) {
  return (
    <OverviewLink
      className={styles.link}
      targetModule={status.target.module}
      query={status.target.query}
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
function Issue({ status }: { status: ResourceStatus }) {
  return (
    <span
      className={`${styles.issueBanner} ${
        status.tone === "error" ? styles.issueError : styles.issueWarning
      }`}
    >
      <ToneMark tone={status.tone} />
      <span>
        {status.label}: {status.detail}
      </span>
    </span>
  );
}

function AgentSummary({
  agent,
  status,
  onNavigate,
}: {
  agent: "codex" | "claude";
  status?: AgentStatus;
  onNavigate: ConsoleNavigate;
}) {
  if (!status) {
    return (
      <div className={styles.sectionContainer}>
        <div className={styles.sectionHeader}>
          <span className={styles.headerTitleGroup}>
            <BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />
            <span className={styles.agentName}>{agentLabel(agent)}</span>
          </span>
        </div>
        <span className={styles.emptyStatusText}>Not reported</span>
      </div>
    );
  }

  const { current, configs, sessions } = status;

  return (
    <div className={styles.sectionContainer}>
      <div className={styles.sectionHeader}>
        <span className={styles.headerTitleGroup}>
          <BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />
          <span className={styles.agentName}>{agentLabel(agent)}</span>
        </span>
      </div>

      <div className={styles.agentConfigRow}>
        <ResourceLink
          status={current}
          onNavigate={onNavigate}
          ariaLabel={status.applicationLabel ?? "Current Config"}
        >
          <span className={styles.currentConfigLink}>
            <span className={styles.currentConfigLabel}>Current Config</span>
            <span className={styles.currentConfigValue}>
              {status.applicationLabel ?? "No recorded application"}
            </span>
          </span>
        </ResourceLink>
      </div>

      <div className={styles.agentChipsRow}>
        <ResourceLink status={configs} onNavigate={onNavigate}>
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
        <ResourceLink status={sessions} onNavigate={onNavigate}>
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
          <Issue status={current} />
        </div>
      )}
    </div>
  );
}

export function TenantStatusList({
  tenants,
  onNavigate,
}: {
  tenants: TenantStatusRow[];
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
        {tenants.map((tenant) => {
          const { components } = tenant;
          const Icon = tenant.kind === "host" ? HostTenantIcon : ManagedTenantIcon;
          const isHost = tenant.kind === "host";
          return (
            <tr key={tenant.id} className={styles.tenantCard}>
              <th scope="row" className={styles.tenantHeaderCell}>
                <div className={styles.tenantIdentity}>
                  <ResourceLink status={tenant} onNavigate={onNavigate}>
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
                  {tenant.detail && <Issue status={tenant} />}
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
                    <ResourceLink status={components} onNavigate={onNavigate}>
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
                    {agentLabel(agent)}
                  </span>
                  <AgentSummary
                    agent={agent}
                    status={tenant.agents[agent]}
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
