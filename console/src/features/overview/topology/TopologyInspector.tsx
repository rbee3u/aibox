import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { AlertTriangle, ArrowUpRight, CheckCircle2, Circle, X } from "lucide-react";
import type { TopologyNode } from "@/features/overview/topology/coreTree";
import styles from "@/features/overview/OverviewPage.module.css";

export function TopologyInspector({
  node,
  onClose,
  onNavigate,
  style,
}: {
  node: TopologyNode;
  onClose: () => void;
  onNavigate: ConsoleNavigate;
  style?: { top: number; left: number };
}) {
  const children = node.children;
  const tone = node.tone;

  return (
    <aside
      className={styles.topologyInspector}
      aria-label={`${node.label} details`}
      data-topology-popover
      style={style}
    >
      <div className={styles.inspectorHeader}>
        <div>
          <span className={styles.inspectorEyebrow}>{labelForIcon(node.icon)}</span>
          <h3>{node.label}</h3>
        </div>
        <button
          type="button"
          className={styles.inspectorClose}
          aria-label="Close details"
          onClick={onClose}
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
      <div className={`${styles.inspectorStatus} ${styles[tone]}`}>
        {tone === "good" ? (
          <CheckCircle2 size={16} />
        ) : tone === "neutral" ? (
          <Circle size={16} />
        ) : (
          <AlertTriangle size={16} />
        )}
        <span>{statusLabel(tone)}</span>
        {node.detail && <small title={node.tooltip ?? node.detail}>{node.detail}</small>}
      </div>
      {node.title && <p className={styles.inspectorEvidence}>{node.title}</p>}
      {children.length > 0 && (
        <div className={styles.inspectorSection}>
          <div className={styles.inspectorSectionHeading}>
            <strong>Resources</strong>
            <span>{children.length}</span>
          </div>
          <div className={styles.inspectorList}>
            {children.map((child) => (
              <div className={styles.inspectorRow} key={child.id}>
                <span>{child.label}</span>
                <small className={styles[child.tone]}>
                  {child.icon === "component"
                    ? componentDetailFromNode(child)
                    : (child.detail ?? statusLabel(child.tone))}
                </small>
              </div>
            ))}
          </div>
        </div>
      )}
      {node.target && (
        <button
          type="button"
          className={styles.inspectorAction}
          onClick={() => onNavigate(node.target!.module, node.target!.query)}
        >
          Open in {capitalize(node.target.module)}
          <ArrowUpRight size={15} aria-hidden="true" />
        </button>
      )}
    </aside>
  );
}

function componentDetailFromNode(node: TopologyNode): string {
  return node.detail ?? statusLabel(node.tone);
}

function labelForIcon(icon: TopologyNode["icon"]): string {
  if (icon === "service") return "Service";
  if (icon === "host" || icon === "tenant") return "Tenant";
  if (icon === "codex" || icon === "claude") return "Coding agent";
  if (icon === "components" || icon === "component") return "Components";
  if (icon === "sessions") return "Sessions";
  return "Configuration";
}

function statusLabel(tone: TopologyNode["tone"]): string {
  if (tone === "good") return "Healthy";
  if (tone === "warning") return "Needs attention";
  if (tone === "error") return "Error";
  return "Neutral";
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
