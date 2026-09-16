import { ArrowUp, ChevronDown, Download, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";
import { createPortal } from "react-dom";
import { useLayoutEffect, useRef, type RefObject } from "react";
import type { ComponentKind, ComponentRow } from "@/api/tenants";
import { ComponentGlyph } from "@/features/tenants/detail/ComponentGlyph";
import { type ComponentRowModel } from "@/features/tenants/componentCatalog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { StatusBadge, type StatusTone, type StatusVariant } from "@/shared/ui/StatusBadge";
import styles from "@/features/tenants/TenantPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

interface ComponentRowItemProps {
  row: ComponentRow;
  model: ComponentRowModel;
  /** Label of the Operation running for this row, if any. */
  progressLabel: string | null;
  busy: boolean;
  mutationBusy: boolean;
  openMenu: ComponentKind | null;
  menuPosition: { top: number; left: number } | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onRetryInspection: () => void;
  onInstall: () => void;
  onRemove: () => void;
  onOpenSpecificVersion: () => void;
  onCloseMenu: () => void;
  onOpenMenu: (anchor: HTMLButtonElement) => void;
  onToggleMenu: (anchor: HTMLButtonElement) => void;
  registerMenuButton: (element: HTMLButtonElement | null) => void;
  registerMenuItem: (element: HTMLButtonElement | null) => void;
  highlighted?: boolean;
}

/**
 * A quiet, non-selectable Component list item: a bare brand icon, a two-line
 * information block, an independent trailing action group, and — only when the
 * row has something to explain — a third line saying why.
 */
export function ComponentRowItem({
  row,
  model,
  progressLabel,
  busy,
  mutationBusy,
  openMenu,
  menuPosition,
  menuRef,
  onRetryInspection,
  onInstall,
  onRemove,
  onOpenSpecificVersion,
  onCloseMenu,
  onOpenMenu,
  onToggleMenu,
  registerMenuButton,
  registerMenuItem,
  highlighted = false,
}: ComponentRowItemProps) {
  const { label, presentation, latest, diagnostic, primaryAction } = model;
  const rowRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!highlighted) return;
    const node = rowRef.current;
    if (!node || typeof node.scrollIntoView !== "function") return;
    const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false;
    node.scrollIntoView({ block: "center", behavior: reduceMotion ? "auto" : "smooth" });
  }, [highlighted]);
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
      ref={rowRef}
      className={`${styles.componentRow} ${progressLabel ? styles.componentRowBusy : ""}`}
      role="listitem"
      data-attention={highlighted ? "true" : undefined}
    >
      <span className={styles.componentIconTile} data-component-icon={row.kind}>
        <ComponentGlyph kind={row.kind} />
      </span>
      <div className={styles.componentContent}>
        <div className={styles.componentIdentity}>
          <strong title={label}>{label}</strong>
        </div>
        <div className={styles.componentMetadata}>
          {progressLabel ? (
            <span className={styles.componentProgress} role="status">
              <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
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
        {presentation.primaryAction === "Retry inspection" ? (
          <ActionButton tone="secondary" disabled={busy} onClick={onRetryInspection}>
            <RefreshCw size={iconSize.xs} aria-hidden="true" />
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
                <ArrowUp size={iconSize.xs} aria-hidden="true" />
              ) : (
                <Download size={iconSize.xs} aria-hidden="true" />
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
              <ChevronDown size={iconSize.xs} />
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
                      <ArrowUp size={iconSize.xs} aria-hidden="true" />
                    ) : (
                      <Download size={iconSize.xs} aria-hidden="true" />
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
              <ArrowUp size={iconSize.xs} aria-hidden="true" />
            ) : (
              <Download size={iconSize.xs} aria-hidden="true" />
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
            <Trash2 size={iconSize.xs} aria-hidden="true" />
          </IconButton>
        )}
      </div>
      {diagnostic && <p className={styles.componentDiagnostic}>{diagnostic}</p>}
    </div>
  );
}
