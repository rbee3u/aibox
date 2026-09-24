import { ChevronLeft, House } from "lucide-react";

import { ComponentCatalogSkeleton } from "@/features/tenants/detail/ComponentCatalogSkeleton";
import { ComponentAgentCard, ComponentRowItem } from "@/features/tenants/detail/ComponentRowItem";
import {
  AGENT_STATUSLINE_KIND,
  componentRowModel,
  relativeTimeLabel,
} from "@/features/tenants/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/tenants/TenantPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const ManagedTenantIcon = resourceIcons.managedTenant;
export function TenantDetailPane({
  components,
  detail,
  dialogs,
  mutations,
  onLocationChange,
  selection,
}: Pick<TenantViewModel, "components" | "detail" | "dialogs" | "mutations" | "selection"> & {
  onLocationChange: ModuleLocationChange;
}) {
  const {
    allComponents,
    attentionKind,
    checkingLatest,
    checkForUpdates,
    componentActionProgress,
    componentCatalogLoading,
    componentGroups,
    closeComponentMenu,
    componentMenuPosition,
    componentMenuRef,
    componentTotalCount,
    differingComponentCount,
    installedComponentCount,
    installComponent,
    issueComponentCount,
    outdatedComponentCount,
    latestSnapshot,
    loadComponents,
    openComponentMenu,
    openMenu,
    openSpecificVersion,
    registerComponentMenuButton,
    registerComponentMenuItem,
    toggleComponentMenu,
  } = components;
  const { detailHeadingRef, selected, selectedKey, tenantKindLabel } = detail;
  const { requestComponentRemove } = dialogs;
  const { busy, mutationBusy } = mutations;
  const { focusTenantRow } = selection;
  return (
    <section className={styles.detailPane}>
      {selected ? (
        <>
          <div
            className={`${styles.detailHeader} ${styles.tenantDetailHeader}`}
            data-component-header
          >
            <div className={styles.componentHeaderInner}>
              <IconButton
                label="Back to Tenants"
                onClick={() => {
                  const focusKey = selectedKey;
                  onLocationChange(new URLSearchParams());
                  window.requestAnimationFrame(() => {
                    if (focusKey) focusTenantRow(focusKey);
                  });
                }}
              >
                <ChevronLeft size={iconSize.md} />
              </IconButton>
              <div className="srOnly">
                <h2 ref={detailHeadingRef} tabIndex={-1}>
                  Components
                </h2>
                <div
                  aria-label={
                    selected.kind === "host"
                      ? "Selected Tenant: Host Tenant"
                      : `Selected Tenant: ${selected.display_name}, ${tenantKindLabel}`
                  }
                >
                  <span>{selected.display_name}</span>
                </div>
              </div>
              <div className={styles.componentHeaderMeta}>
                <div className={styles.componentSummary} aria-label="Component summary">
                  {componentCatalogLoading ? (
                    <span className={styles.componentHeaderLoading}>Loading…</span>
                  ) : (
                    <>
                      <span
                        className={styles.componentInstalledSummary}
                        data-status={
                          installedComponentCount === 0
                            ? "none"
                            : installedComponentCount === componentTotalCount
                              ? "all"
                              : "partial"
                        }
                      >
                        <span>
                          <strong>{installedComponentCount}</strong>/{componentTotalCount} installed
                        </span>
                      </span>
                      {outdatedComponentCount > 0 && (
                        <span className={styles.componentSummaryOutdated}>
                          {outdatedComponentCount} outdated
                        </span>
                      )}
                      {differingComponentCount > 0 && (
                        <span className={styles.componentSummaryDiffers}>
                          {differingComponentCount} differs
                        </span>
                      )}
                      {issueComponentCount > 0 && (
                        <span className={styles.componentSummaryIssue}>
                          {issueComponentCount} {issueComponentCount === 1 ? "issue" : "issues"}
                        </span>
                      )}
                    </>
                  )}
                </div>
                {latestSnapshot && (
                  <span className={styles.componentCheckedAt}>
                    Checked {relativeTimeLabel(latestSnapshot.checked_at)}
                  </span>
                )}
                <RefreshButton
                  className={styles.componentCheckButton}
                  tone="ghost"
                  compactOnNarrow
                  /*
                   * The accessible name keeps the freshness unconditionally.
                   * The visible node beside this button carries it wherever
                   * there is room, and drops out on a narrow pane — so the name
                   * is the one path that is never the width's to take away.
                   */
                  label={
                    latestSnapshot
                      ? `Check for updates, checked ${relativeTimeLabel(latestSnapshot.checked_at)}`
                      : "Check for updates"
                  }
                  busy={checkingLatest}
                  busyLabel="Checking for updates"
                  disabled={checkingLatest}
                  onClick={() => void checkForUpdates()}
                >
                  Check for updates
                </RefreshButton>
              </div>
            </div>
          </div>
          <div
            className={styles.componentViewport}
            aria-busy={componentCatalogLoading || undefined}
          >
            <div className={styles.componentCatalogContent}>
              {componentCatalogLoading ? (
                <ComponentCatalogSkeleton host={selected.kind === "host"} />
              ) : (
                <div className={styles.componentCatalog} aria-label="Components">
                  {componentGroups.map((group) => {
                    const groupCount =
                      group.id === "agents" ? group.rows.length * 2 : group.rows.length;
                    return (
                      <section
                        className={styles.componentGroup}
                        aria-labelledby={`component-group-${group.id}`}
                        key={group.id}
                      >
                        <div className={styles.componentGroupHeader}>
                          <h3 id={`component-group-${group.id}`}>{group.label}</h3>
                          <span className={styles.componentGroupCount}>
                            {groupCount} {groupCount === 1 ? "component" : "components"}
                          </span>
                        </div>
                        <div
                          role="list"
                          aria-label={`${group.label} Components`}
                          className={styles.componentGrid}
                        >
                          {group.id === "agents"
                            ? group.rows.map((row) => {
                                const model = componentRowModel(row, latestSnapshot);
                                const rowProgress =
                                  componentActionProgress?.tenantSelectionValue === selectedKey &&
                                  componentActionProgress.kind === row.kind
                                    ? componentActionProgress.label
                                    : null;
                                const statuslineKind =
                                  row.kind in AGENT_STATUSLINE_KIND
                                    ? AGENT_STATUSLINE_KIND[row.kind as "codex" | "claude"]
                                    : null;
                                const statuslineRow = statuslineKind
                                  ? (allComponents.find((r) => r.kind === statuslineKind) ?? null)
                                  : null;
                                const statuslineModel = statuslineRow
                                  ? componentRowModel(statuslineRow, latestSnapshot)
                                  : null;
                                const statuslineProgress =
                                  statuslineRow &&
                                  componentActionProgress?.tenantSelectionValue === selectedKey &&
                                  componentActionProgress.kind === statuslineRow.kind
                                    ? componentActionProgress.label
                                    : null;

                                if (row.kind !== "codex" && row.kind !== "claude") {
                                  return (
                                    <ComponentRowItem
                                      key={row.kind}
                                      row={row}
                                      model={model}
                                      highlighted={attentionKind === row.kind}
                                      progressLabel={rowProgress}
                                      busy={busy}
                                      mutationBusy={mutationBusy}
                                      openMenu={openMenu}
                                      menuPosition={componentMenuPosition}
                                      menuRef={componentMenuRef}
                                      onRetryInspection={() => void loadComponents(selected)}
                                      onInstall={(version) => installComponent(row, version)}
                                      onRemove={() =>
                                        requestComponentRemove(row, selected.display_name)
                                      }
                                      onOpenSpecificVersion={() =>
                                        openSpecificVersion(row, model.specificVersionMode)
                                      }
                                      onCloseMenu={closeComponentMenu}
                                      onOpenMenu={(anchor) =>
                                        openComponentMenu(row.kind, anchor, model.menuWidth)
                                      }
                                      onToggleMenu={(anchor) =>
                                        toggleComponentMenu(row.kind, anchor, model.menuWidth)
                                      }
                                      registerMenuButton={(element) =>
                                        registerComponentMenuButton(row.kind, element)
                                      }
                                      registerMenuItem={(element) =>
                                        registerComponentMenuItem(row.kind, element)
                                      }
                                    />
                                  );
                                }

                                return (
                                  <ComponentAgentCard
                                    key={row.kind}
                                    agent={{
                                      row,
                                      model,
                                      highlighted: attentionKind === row.kind,
                                      progressLabel: rowProgress,
                                      busy,
                                      mutationBusy,
                                      openMenu,
                                      menuPosition: componentMenuPosition,
                                      menuRef: componentMenuRef,
                                      onRetryInspection: () => void loadComponents(selected),
                                      onInstall: (version) => installComponent(row, version),
                                      onRemove: () =>
                                        requestComponentRemove(row, selected.display_name),
                                      onOpenSpecificVersion: () =>
                                        openSpecificVersion(row, model.specificVersionMode),
                                      onCloseMenu: closeComponentMenu,
                                      onOpenMenu: (anchor) =>
                                        openComponentMenu(row.kind, anchor, model.menuWidth),
                                      onToggleMenu: (anchor) =>
                                        toggleComponentMenu(row.kind, anchor, model.menuWidth),
                                      registerMenuButton: (element) =>
                                        registerComponentMenuButton(row.kind, element),
                                      registerMenuItem: (element) =>
                                        registerComponentMenuItem(row.kind, element),
                                    }}
                                    statusline={
                                      statuslineRow && statuslineModel
                                        ? {
                                            row: statuslineRow,
                                            model: statuslineModel,
                                            progressLabel: statuslineProgress,
                                            busy,
                                            mutationBusy,
                                            highlighted: attentionKind === statuslineRow.kind,
                                            onRetryInspection: () => void loadComponents(selected),
                                            onInstall: () => installComponent(statuslineRow),
                                            onRemove: () =>
                                              requestComponentRemove(
                                                statuslineRow,
                                                selected.display_name,
                                              ),
                                          }
                                        : null
                                    }
                                  />
                                );
                              })
                            : group.rows.map((row) => {
                                const model = componentRowModel(row, latestSnapshot);
                                const rowProgress =
                                  componentActionProgress?.tenantSelectionValue === selectedKey &&
                                  componentActionProgress.kind === row.kind
                                    ? componentActionProgress.label
                                    : null;
                                return (
                                  <ComponentRowItem
                                    key={row.kind}
                                    row={row}
                                    model={model}
                                    highlighted={attentionKind === row.kind}
                                    progressLabel={rowProgress}
                                    busy={busy}
                                    mutationBusy={mutationBusy}
                                    openMenu={openMenu}
                                    menuPosition={componentMenuPosition}
                                    menuRef={componentMenuRef}
                                    onRetryInspection={() => void loadComponents(selected)}
                                    onInstall={(version) => installComponent(row, version)}
                                    onRemove={() =>
                                      requestComponentRemove(row, selected.display_name)
                                    }
                                    onOpenSpecificVersion={() =>
                                      openSpecificVersion(row, model.specificVersionMode)
                                    }
                                    onCloseMenu={closeComponentMenu}
                                    onOpenMenu={(anchor) =>
                                      openComponentMenu(row.kind, anchor, model.menuWidth)
                                    }
                                    onToggleMenu={(anchor) =>
                                      toggleComponentMenu(row.kind, anchor, model.menuWidth)
                                    }
                                    registerMenuButton={(element) =>
                                      registerComponentMenuButton(row.kind, element)
                                    }
                                    registerMenuItem={(element) =>
                                      registerComponentMenuItem(row.kind, element)
                                    }
                                  />
                                );
                              })}
                        </div>
                      </section>
                    );
                  })}
                </div>
              )}
              {!componentCatalogLoading && selected.kind === "host" && (
                <div className={styles.hostEnvironmentNotice}>
                  <div className={styles.hostNoticeHeader}>
                    <House
                      size={iconSize.sm}
                      className={styles.hostNoticeIcon}
                      aria-hidden="true"
                    />
                    <strong>Host Workstation Environment</strong>
                  </div>
                  <p>
                    The Host Tenant operates directly on your local workstation without
                    containerization. AIBox manages statusline integration here, while Agents
                    (Claude & Codex) and Toolchain runtimes remain isolated within containerized
                    Managed Tenants.
                  </p>
                </div>
              )}
            </div>
          </div>
        </>
      ) : (
        <EmptyState
          variant="detail"
          icon={<ManagedTenantIcon size={iconSize.xl} aria-hidden="true" />}
          title="Select a Tenant"
          description="Choose a Tenant to inspect its Components."
        />
      )}
    </section>
  );
}
