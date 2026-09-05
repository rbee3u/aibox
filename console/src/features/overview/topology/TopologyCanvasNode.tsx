import { Minus, Plus } from "lucide-react";
import { useId, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import {
  topologyNodeDisclosure,
  type SessionLoad,
  type TopologyLayoutNode,
  type TopologyNode,
  type TreeIcon,
} from "@/features/overview/topology/topologyModel";
import type { Tone } from "@/features/overview/viewTypes";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { moduleIcons, resourceIcons } from "@/shared/icons/consoleIcons";
import { AnchoredTooltip } from "@/shared/ui/AnchoredTooltip";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/overview/OverviewPage.module.css";

const NODE_TOOLTIP_DELAY_MS = 150;

const ComponentGroupIcon = resourceIcons.components;
const ComponentIcon = resourceIcons.component;
const ConfigsModuleIcon = moduleIcons.configs;
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
const ServiceIcon = resourceIcons.service;
const SessionsModuleIcon = moduleIcons.sessions;

interface TopologyCanvasNodeProps {
  layoutNode: TopologyLayoutNode;
  active: boolean;
  selected: boolean;
  traced: boolean;
  forcedOpen: boolean;
  load?: SessionLoad;
  registerNode: (id: string, element: HTMLDivElement | null) => void;
  onFocus: (id: string) => void;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
  onKeyDown: (event: KeyboardEvent<HTMLDivElement>, node: TopologyNode) => void;
  onToggle: (node: TopologyNode) => void;
  onRefreshSession: (node: TopologyNode) => void;
}
export function TopologyCanvasNode(props: TopologyCanvasNodeProps) {
  const { layoutNode } = props;
  const { node } = layoutNode;
  const descriptionId = useId();
  const disclosure = topologyNodeDisclosure(node);
  const content = (
    <>
      <span className={styles.nodeIcon} data-tree-icon={node.icon}>
        {treeIcon(node.icon)}
      </span>
      <span className={styles.nodeCopy}>
        <strong>{node.label}</strong>
        {node.detail && <small>{node.detail}</small>}
      </span>
      <StatusMark tone={node.tone} />
    </>
  );
  return (
    <AnchoredTooltip
      disabled={!disclosure || props.selected}
      openDelayMs={NODE_TOOLTIP_DELAY_MS}
      className={styles.nodeTooltip}
      positionKey={node.id}
      content={disclosure}
    >
      {(tooltip) => (
        <div
          ref={(element) => {
            tooltip.triggerRef.current = element;
            props.registerNode(node.id, element);
          }}
          className={`${styles.topologyNode} ${styles[layoutNode.kind]} ${styles[node.tone]} ${props.active ? styles.nodeActive : ""} ${props.traced ? styles.nodeTraced : ""}`}
          style={{
            left: layoutNode.x,
            top: layoutNode.y,
            width: layoutNode.width,
            height: layoutNode.height,
          }}
          role="treeitem"
          aria-label={node.detail ? `${node.label} ${node.detail}` : node.label}
          aria-describedby={disclosure ? descriptionId : undefined}
          aria-level={layoutNode.depth + 1}
          aria-posinset={layoutNode.position}
          aria-setsize={layoutNode.setSize}
          aria-expanded={layoutNode.branch ? layoutNode.open : undefined}
          tabIndex={props.active ? 0 : -1}
          data-node-id={node.id}
          data-node-kind={layoutNode.kind}
          onPointerEnter={(event) => {
            tooltip.onPointerEnter(event);
            props.onHover(node.id);
          }}
          onPointerLeave={(event) => {
            tooltip.onPointerLeave(event);
            props.onHover(null);
          }}
          onPointerDown={(event) => {
            tooltip.close();
            tooltip.onPointerDown(event);
          }}
          onFocus={(event) => {
            if (event.target === event.currentTarget) props.onFocus(node.id);
            if (disclosure && !props.selected && event.currentTarget.matches(":focus-visible")) {
              tooltip.onFocus(event);
            }
          }}
          onBlur={tooltip.onBlur}
          onClick={() => props.onSelect(node.id)}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") tooltip.close();
            if (disclosure && !props.selected) tooltip.onKeyDown(event);
            props.onKeyDown(event, node);
          }}
        >
          {disclosure && (
            <span id={descriptionId} className="srOnly">
              {disclosure}
            </span>
          )}
          {node.parentId && <span className={styles.inputPort} aria-hidden="true" />}
          <div className={styles.nodeSurface}>{content}</div>
          {node.sessionRequest && (
            <RefreshButton
              type="button"
              className={styles.topologySessionRefresh}
              label={`Refresh ${node.label} summary`}
              iconOnly
              iconSize={12}
              tabIndex={-1}
              busy={props.load?.state === "loading"}
              disabled={props.load?.state === "loading"}
              onMouseDown={keepTreeitemFocus}
              onClick={(event) => {
                event.stopPropagation();
                props.onRefreshSession(node);
              }}
            />
          )}
          {layoutNode.branch && node.id !== "service" ? (
            <button
              type="button"
              className={styles.disclosure}
              tabIndex={-1}
              aria-label={`${layoutNode.open ? "Collapse" : "Expand"} ${node.label}`}
              aria-expanded={layoutNode.open}
              disabled={props.forcedOpen}
              title={
                props.forcedOpen ? "Clear the active filter to collapse this branch" : undefined
              }
              onMouseDown={keepTreeitemFocus}
              onClick={(event) => {
                event.stopPropagation();
                props.onFocus(node.id);
                props.onToggle(node);
              }}
            >
              {layoutNode.open ? <Minus size={13} /> : <Plus size={13} />}
            </button>
          ) : layoutNode.branch ? (
            <span className={styles.outputPort} aria-hidden="true" />
          ) : null}
        </div>
      )}
    </AnchoredTooltip>
  );
}
function keepTreeitemFocus(event: MouseEvent<HTMLElement>) {
  event.preventDefault();
  event.currentTarget.closest<HTMLElement>("[role='treeitem']")?.focus({ preventScroll: true });
}

function StatusMark({ tone }: { tone: Tone }) {
  const label = statusLabel(tone);
  return (
    <span className={`${styles.statusMark} ${styles[tone]}`} title={label} aria-label={label} />
  );
}
function statusLabel(tone: Tone): string {
  switch (tone) {
    case "good":
      return "Healthy";
    case "warning":
      return "Needs attention";
    case "error":
      return "Error";
    case "neutral":
      return "Neutral";
  }
}
function treeIcon(icon: TreeIcon): ReactNode {
  switch (icon) {
    case "service":
      return <ServiceIcon size={17} />;
    case "host":
      return <HostTenantIcon size={16} />;
    case "tenant":
      return <ManagedTenantIcon size={16} />;
    case "codex":
      return <BrandIcon brand={brandForAgent("codex")} size={16} />;
    case "claude":
      return <BrandIcon brand={brandForAgent("claude")} size={16} />;
    case "current":
      return <CurrentConfigIcon size={15} />;
    case "configs":
      return <ConfigsModuleIcon size={15} />;
    case "config":
      return <NamedConfigIcon size={15} />;
    case "sessions":
      return <SessionsModuleIcon size={15} />;
    case "components":
      return <ComponentGroupIcon size={15} />;
    case "component":
      return <ComponentIcon size={15} />;
  }
}
