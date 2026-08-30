import {
  ArrowUp,
  ChevronDown,
  ChevronUp,
  Download,
  LoaderCircle,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { createPortal } from "react-dom";
import type { RefObject } from "react";
import type { ComponentKind, ComponentRow } from "@/api/tenants";
import { ComponentGlyph } from "@/features/tenants/detail/ComponentGlyph";
import {
  type ComponentBadgeTone,
  type ComponentPrimaryAction,
  type ComponentRowModel,
} from "@/features/tenants/componentCatalog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";

const BADGE_TONE_CLASS: Record<ComponentBadgeTone, string> = {
  warn: layout.statusWarn,
  error: layout.statusError,
};

function actionClass(action: ComponentPrimaryAction): string {
  switch (action) {
    case "Install":
      return styles.componentInstallAction;
    case "Update":
      return styles.componentUpdateAction;
    case "Repair":
    case "Restore":
      return styles.componentRepairAction;
    case "Retry inspection":
      return styles.componentRetryAction;
  }
}

interface ComponentRowItemProps {
  row: ComponentRow;
  model: ComponentRowModel;
  expanded: boolean;
  /** Label of the Operation running for this row, if any. */
  progressLabel: string | null;
  busy: boolean;
  mutationBusy: boolean;
  openMenu: ComponentKind | null;
  menuPosition: { top: number; left: number } | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onToggleExpanded: () => void;
  onRetryInspection: () => void;
  onInstall: () => void;
  onRemove: () => void;
  onOpenSpecificVersion: () => void;
  onCloseMenu: () => void;
  onOpenMenu: (anchor: HTMLButtonElement) => void;
  onToggleMenu: (anchor: HTMLButtonElement) => void;
  registerMenuButton: (element: HTMLButtonElement | null) => void;
  registerMenuItem: (element: HTMLButtonElement | null) => void;
}

/**
 * A quiet, non-selectable Component list item: a bare brand icon, a fixed
 * two-line information block, and an independent trailing action group.
 */
export function ComponentRowItem({
  row,
  model,
  expanded,
  progressLabel,
  busy,
  mutationBusy,
  openMenu,
  menuPosition,
  menuRef,
  onToggleExpanded,
  onRetryInspection,
  onInstall,
  onRemove,
  onOpenSpecificVersion,
  onCloseMenu,
  onOpenMenu,
  onToggleMenu,
  registerMenuButton,
  registerMenuItem,
}: ComponentRowItemProps) {
  const { label, presentation, latest, diagnostic, primaryAction } = model;
  const menuOpen = openMenu === row.kind;

  return (
    <div
      className={`${styles.componentRow} ${progressLabel ? styles.componentRowBusy : ""}`}
      role="listitem"
    >
      <span className={styles.componentIconTile} data-component-icon={row.kind}>
        <ComponentGlyph kind={row.kind} />
      </span>
      <div className={styles.componentContent}>
        <div className={styles.componentIdentity}>
          <strong>{label}</strong>
        </div>
        <div className={styles.componentMetadata}>
          {progressLabel ? (
            <span className={styles.componentProgress} role="status">
              <LoaderCircle className="spin" size={14} aria-hidden="true" />
              {progressLabel}
            </span>
          ) : (
            <>
              <div className={styles.componentState} aria-label={`${label} installed state`}>
                <span className={styles.componentStateValue}>
                  {presentation.stateBadge && presentation.badgeTone ? (
                    <span
                      className={`${BADGE_TONE_CLASS[presentation.badgeTone]} ${styles.componentStateBadge}`}
                    >
                      {presentation.stateBadge}
                    </span>
                  ) : (
                    presentation.stateLabel
                  )}
                  {row.version && (row.status === "installed" || row.status === "modified") && (
                    <strong>v{row.version}</strong>
                  )}
                </span>
              </div>
              {model.showLatest && (
                <>
                  <span className={styles.componentInlineSeparator} aria-hidden="true">
                    ·
                  </span>
                  <div className={styles.componentRelease} aria-label={`${label} latest release`}>
                    {latest.latestVersion ? (
                      <span className={styles.componentReleaseValue}>
                        Latest <strong>v{latest.latestVersion}</strong>
                      </span>
                    ) : (
                      <span className={styles.componentVersionUnavailable}>{latest.label}</span>
                    )}
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </div>
      <div className={styles.componentActions}>
        {diagnostic && (
          <ActionButton
            tone="quiet"
            className={styles.componentDetailsButton}
            aria-expanded={expanded}
            aria-controls={`component-diagnostic-${row.kind}`}
            onClick={onToggleExpanded}
          >
            {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
            Details
          </ActionButton>
        )}
        {presentation.primaryAction === "Retry inspection" ? (
          <ActionButton
            tone="default"
            className={styles.componentRetryAction}
            disabled={busy}
            onClick={onRetryInspection}
          >
            <RefreshCw size={14} aria-hidden="true" />
            Retry inspection
          </ActionButton>
        ) : primaryAction && model.canSpecificVersion ? (
          <div className={styles.componentSplitAction}>
            <ActionButton
              tone="default"
              className={`${styles.componentSplitPrimary} ${actionClass(primaryAction)}`}
              disabled={mutationBusy}
              onClick={onInstall}
            >
              {primaryAction === "Update" ? (
                <ArrowUp size={14} aria-hidden="true" />
              ) : (
                <Download size={14} aria-hidden="true" />
              )}{" "}
              {primaryAction}
            </ActionButton>
            <ActionButton
              ref={registerMenuButton}
              tone="default"
              className={`${styles.componentSplitTrigger} ${actionClass(primaryAction)}`}
              aria-label={`${primaryAction} options for ${label}`}
              aria-controls={menuOpen ? `component-install-menu-${row.kind}` : undefined}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              disabled={mutationBusy}
              onClick={(event) => {
                onToggleMenu(event.currentTarget);
              }}
              onKeyDown={(event) => {
                if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
                event.preventDefault();
                onOpenMenu(event.currentTarget);
              }}
            >
              <ChevronDown size={14} />
            </ActionButton>
            {menuOpen &&
              createPortal(
                <div
                  id={`component-install-menu-${row.kind}`}
                  ref={menuRef}
                  className={styles.componentMoreMenu}
                  style={{
                    top: menuPosition?.top ?? 0,
                    left: menuPosition?.left ?? 0,
                    width: model.menuWidth,
                  }}
                  role="menu"
                  aria-label={`${label} ${model.specificVersionMode} options`}
                >
                  <button
                    ref={registerMenuItem}
                    type="button"
                    role="menuitem"
                    onKeyDown={(event) => {
                      if (event.key === "Tab") onCloseMenu();
                    }}
                    onClick={() => {
                      onCloseMenu();
                      onOpenSpecificVersion();
                    }}
                  >
                    {model.specificVersionMode === "update" ? (
                      <ArrowUp size={14} aria-hidden="true" />
                    ) : (
                      <Download size={14} aria-hidden="true" />
                    )}
                    {model.specificVersionMode === "update"
                      ? "Update to version…"
                      : "Install version…"}
                  </button>
                </div>,
                document.body,
              )}
          </div>
        ) : primaryAction ? (
          <ActionButton
            tone="default"
            className={`${styles.componentPrimaryAction} ${actionClass(primaryAction)}`}
            disabled={mutationBusy}
            onClick={onInstall}
          >
            {primaryAction === "Update" ? (
              <ArrowUp size={14} aria-hidden="true" />
            ) : (
              <Download size={14} aria-hidden="true" />
            )}{" "}
            {primaryAction}
          </ActionButton>
        ) : null}
        {presentation.canRemove && (
          <IconButton
            className={styles.componentRemoveAction}
            label={`Remove ${label}`}
            disabled={mutationBusy}
            onClick={onRemove}
          >
            <Trash2 size={15} aria-hidden="true" />
          </IconButton>
        )}
      </div>
      {expanded && diagnostic && (
        <div id={`component-diagnostic-${row.kind}`} className={styles.componentDiagnostic}>
          {diagnostic}
        </div>
      )}
    </div>
  );
}
