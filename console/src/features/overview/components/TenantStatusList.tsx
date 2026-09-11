import { Box, House } from "lucide-react";
import type { ReactNode } from "react";
import { OverviewLink } from "@/features/overview/OverviewLink";
import type { TopologyNode } from "@/features/overview/resourceTree";
import type { Tone } from "@/features/overview/viewTypes";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import styles from "@/features/overview/components/TenantStatusList.module.css";
import { toneIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";

/**
 * The mark for a tone that needs work, or nothing when it does not.
 *
 * A warning and an error differ in shape here for the same reason they do in
 * the attention panel: the two tones are a hue apart, which is no difference
 * at all for a red-green colour deficiency, and this table renders both.
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
 * A status fact stated in place, not a second way to reach the same page.
 *
 * This used to wrap itself in a link to the node it describes — the same node
 * the cell's own link already targets, so "Current Config: Differs" and
 * "Home unavailable" were duplicate tab stops landing where their neighbour
 * landed. A fact is not an action; the action stays one per destination.
 */
function Issue({ node }: { node: TopologyNode }) {
  return (
    <span className={`${styles.fact} ${node.tone === "error" ? styles.error : styles.warning}`}>
      <ToneMark tone={node.tone} />
      <span>
        {node.label}: {node.detail}
      </span>
    </span>
  );
}

function AgentSummary({ node, onNavigate }: { node?: TopologyNode; onNavigate: ConsoleNavigate }) {
  if (!node) return <span className={styles.secondary}>Not reported</span>;
  const current = node.children.find((child) => child.icon === "current")!;
  const configs = node.children.find((child) => child.icon === "configs")!;
  const sessions = node.children.find((child) => child.icon === "sessions")!;
  return (
    <div className={styles.summary}>
      <ResourceLink
        node={current}
        onNavigate={onNavigate}
        ariaLabel={node.detail ?? "Current Config"}
      >
        <span className={styles.primaryLink}>Current Config →</span>
        <span className={styles.secondary}>{node.detail ?? "No recorded application"}</span>
      </ResourceLink>
      <span
        className={
          configs.tone === "error"
            ? styles.error
            : configs.tone === "warning"
              ? styles.warning
              : styles.secondary
        }
      >
        <ResourceLink node={configs} onNavigate={onNavigate}>
          <ToneMark tone={configs.tone} />
          <span className={styles.secondaryLink}>{configs.detail}</span>
        </ResourceLink>
      </span>
      <span className={sessions.tone === "error" ? styles.error : styles.secondary}>
        <ResourceLink node={sessions} onNavigate={onNavigate}>
          <ToneMark tone={sessions.tone} />
          <span className={styles.secondaryLink}>{sessions.detail}</span>
        </ResourceLink>
      </span>
      {current.tone !== "neutral" && <Issue node={current} />}
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
          <th scope="col">Codex</th>
          <th scope="col">Claude</th>
          <th scope="col">Components</th>
        </tr>
      </thead>
      <tbody>
        {root.children.map((tenant) => {
          const components = tenant.children.find((child) => child.icon === "components")!;
          const Icon = tenant.icon === "host" ? House : Box;
          return (
            <tr key={tenant.id}>
              <th scope="row">
                <div className={styles.summary}>
                  <ResourceLink node={tenant} onNavigate={onNavigate}>
                    <Icon size={iconSize.md} aria-hidden="true" />
                    <strong>{tenant.label}</strong>
                  </ResourceLink>
                  <span className={styles.secondary}>
                    {tenant.icon === "host" ? "Host · Management only" : "Managed Tenant"}
                  </span>
                  {tenant.detail && <Issue node={tenant} />}
                </div>
              </th>
              {(["codex", "claude"] as const).map((agent) => (
                <td key={agent}>
                  <span className={styles.mobileLabel} aria-hidden="true">
                    {agent === "codex" ? "Codex" : "Claude"}
                  </span>
                  <AgentSummary
                    node={tenant.children.find((child) => child.icon === agent)}
                    onNavigate={onNavigate}
                  />
                </td>
              ))}
              <td>
                <span className={styles.mobileLabel} aria-hidden="true">
                  Components
                </span>
                <div className={styles.summary}>
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
                    <span>{components.detail}</span>
                  </span>
                  <ResourceLink node={components} onNavigate={onNavigate}>
                    <span className={styles.primaryLink}>Manage components →</span>
                  </ResourceLink>
                </div>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
