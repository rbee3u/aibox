import { ArrowUp, ChevronDown, Download, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";
import { createPortal } from "react-dom";
import type { RefObject } from "react";
import type { ComponentKind, ComponentRow } from "@/api/tenants";
import { ComponentGlyph } from "@/features/tenants/detail/ComponentGlyph";
import { type ComponentRowModel } from "@/features/tenants/componentCatalog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { StatusBadge, type StatusTone, type StatusVariant } from "@/shared/ui/StatusBadge";
import styles from "@/features/tenants/TenantPage.module.css";

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
  const stateTone: StatusTone =
    row.error || !row.status
      ? "error"
      : row.status === "installed"
        ? "good"
        : row.status === "not-installed"
          ? "neutral"
          : "warning";
  const stateVariant: StatusVariant =
    stateTone === "good" || stateTone === "neutral" ? "inline" : "badge";

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
                  <StatusBadge tone={stateTone} variant={stateVariant}>
                    {presentation.stateBadge ?? presentation.stateLabel}
                  </StatusBadge>
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
            tone="ghost"
            className={styles.componentDetailsButton}
            aria-expanded={expanded}
            aria-controls={`component-diagnostic-${row.kind}`}
            onClick={onToggleExpanded}
          >
            Details
          </ActionButton>
        )}
        {presentation.primaryAction === "Retry inspection" ? (
          <ActionButton tone="secondary" disabled={busy} onClick={onRetryInspection}>
            <RefreshCw size={14} aria-hidden="true" />
            Retry inspection
          </ActionButton>
        ) : primaryAction && model.canSpecificVersion ? (
          <div className={styles.componentSplitAction}>
            <ActionButton
              tone="primarySoft"
              className={styles.componentSplitPrimary}
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
              tone="primarySoft"
              className={styles.componentSplitTrigger}
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
            tone="primarySoft"
            className={styles.componentPrimaryAction}
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
            tone="dangerQuiet"
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
