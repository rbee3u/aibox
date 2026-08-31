import { Check, ChevronLeft, Clipboard } from "lucide-react";

import { ComponentCatalogSkeleton } from "@/features/tenants/detail/ComponentCatalogSkeleton";
import { ComponentRowItem } from "@/features/tenants/detail/ComponentRowItem";
import { componentRowModel, relativeTimeLabel } from "@/features/tenants/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/tenants/TenantPage.module.css";

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
    attentionComponentCount,
    checkingLatest,
    checkForUpdates,
    componentActionProgress,
    componentCatalogLoading,
    componentGroups,
    closeComponentMenu,
    componentMenuPosition,
    componentMenuRef,
    componentTotalCount,
    installedComponentCount,
    isComponentExpanded,
    latestSnapshot,
    loadComponents,
    mutateComponent,
    openComponentMenu,
    openMenu,
    openSpecificVersion,
    registerComponentMenuButton,
    registerComponentMenuItem,
    toggleComponentExpanded,
    toggleComponentMenu,
  } = components;
  const {
    copiedHome,
    copyHome,
    detailHeadingRef,
    selected,
    selectedHome,
    selectedKey,
    tenantKindLabel,
  } = detail;
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
                <ChevronLeft size={17} />
              </IconButton>
              <div className={styles.componentHeaderIdentity}>
                <h2 ref={detailHeadingRef} tabIndex={-1}>
                  Components
                </h2>
                <div
                  className={styles.componentHeaderContext}
                  aria-label={
                    selected.kind === "host"
                      ? "Selected Tenant: Host Tenant"
                      : `Selected Tenant: ${selected.display_name}, ${tenantKindLabel}`
                  }
                >
                  <span className={styles.componentTenant}>{selected.display_name}</span>
                  <div className={styles.componentHome}>
                    <span aria-hidden="true">·</span>
                    <code title={selected.home}>{selectedHome}</code>
                    <IconButton
                      className={styles.componentHomeCopy}
                      label={
                        copiedHome === selected.home ? "Tenant Home copied" : "Copy Tenant Home"
                      }
                      onClick={() => void copyHome(selected.home, selected.home)}
                    >
                      {copiedHome === selected.home ? <Check size={13} /> : <Clipboard size={13} />}
                    </IconButton>
                  </div>
                </div>
              </div>
              <div className={styles.componentHeaderMeta} aria-label="Component summary">
                {componentCatalogLoading ? (
                  <span className={styles.componentHeaderLoading}>Loading…</span>
                ) : (
                  <>
                    <span className={styles.componentInstalledSummary}>
                      <strong>{installedComponentCount}</strong>/{componentTotalCount} installed
                    </span>
                    {attentionComponentCount > 0 && (
                      <span className={styles.componentSummaryAttention}>
                        {attentionComponentCount}{" "}
                        {attentionComponentCount === 1 ? "issue" : "issues"}
                      </span>
                    )}
                  </>
                )}
                <div className={styles.componentCheckStatus}>
                  {latestSnapshot ? (
                    <time
                      dateTime={latestSnapshot.checked_at}
                      title={new Date(latestSnapshot.checked_at).toLocaleString()}
                    >
                      Checked {relativeTimeLabel(latestSnapshot.checked_at)}
                    </time>
                  ) : (
                    <span>Not checked</span>
                  )}
                </div>
                <RefreshButton
                  className={styles.componentCheckButton}
                  label="Check for updates"
                  busy={checkingLatest}
                  busyLabel="Checking for updates"
                  disabled={checkingLatest}
                  onClick={() => void checkForUpdates()}
                />
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
                  {componentGroups.map((group) => (
                    <section
                      className={styles.componentGroup}
                      aria-labelledby={`component-group-${group.id}`}
                      key={group.id}
                    >
                      <div className={styles.componentGroupHeader}>
                        <h3 id={`component-group-${group.id}`}>{group.label}</h3>
                      </div>
                      <div role="list" aria-label={`${group.label} Components`}>
                        {group.rows.map((row) => {
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
                              expanded={isComponentExpanded(row.kind)}
                              progressLabel={rowProgress}
                              busy={busy}
                              mutationBusy={mutationBusy}
                              openMenu={openMenu}
                              menuPosition={componentMenuPosition}
                              menuRef={componentMenuRef}
                              onToggleExpanded={() => toggleComponentExpanded(row.kind)}
                              onRetryInspection={() => void loadComponents(selected)}
                              onInstall={() => void mutateComponent(row, true)}
                              onRemove={() => requestComponentRemove(row, selected.display_name)}
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
                  ))}
                </div>
              )}
            </div>
          </div>
        </>
      ) : (
        <EmptyState
          variant="detail"
          icon={<ManagedTenantIcon size={26} aria-hidden="true" />}
          title="Select a Tenant"
          description="Choose a Tenant to inspect its Components."
        />
      )}
    </section>
  );
}
