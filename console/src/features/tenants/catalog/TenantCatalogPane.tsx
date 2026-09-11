import { Check, ListChecks, Plus, Trash2 } from "lucide-react";

import { tenantLocation, tenantSelectionValueOf } from "@/features/tenants/route";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { catalogMarksInspection, useNarrowLayout } from "@/shared/hooks/useNarrowLayout";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { EmptyState } from "@/shared/ui/EmptyState";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconLabelButton } from "@/shared/ui/IconLabelButton";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
export function TenantCatalogPane({
  catalog,
  detail,
  dialogs,
  feedback,
  mutations,
  onLocationChange,
  selection,
}: Pick<
  TenantViewModel,
  "catalog" | "detail" | "dialogs" | "feedback" | "mutations" | "selection"
> & {
  onLocationChange: ModuleLocationChange;
}) {
  const {
    hostTenant,
    loadingTenants,
    managedTenants,
    refreshing,
    refreshTenants,
    tenantCatalogError,
  } = catalog;
  const { detailOpen, selectedKey } = detail;
  const narrowLayout = useNarrowLayout();
  const inspectedKey = catalogMarksInspection(narrowLayout, detailOpen) ? selectedKey : null;
  const { openCreateDialog } = dialogs;
  const { busy, mutationBusy, requestTenantDelete } = mutations;
  const {
    allSelectable,
    cancelSelection,
    selectedCount,
    selectedKeys,
    selectableKeys,
    selectionMode,
    enterSelection,
    registerTenantRow,
    toggleAllTenants,
    toggleTenant,
  } = selection;
  return (
    <aside className={styles.tenantCatalog} aria-label="Tenants">
      <div className={`${layout.toolbar} ${selectionMode ? layout.selectionBar : ""}`}>
        {selectionMode ? (
          <>
            <ActionButton
              tone="ghost"
              className={layout.selectionCancel}
              disabled={busy}
              onClick={cancelSelection}
            >
              Cancel
            </ActionButton>
            <div className={layout.selectionCenter}>
              <span className={layout.selectionCount}>{selectedCount} selected</span>
              <ActionButton
                tone="ghost"
                className={layout.selectionAll}
                disabled={selectableKeys.length === 0 || busy}
                onClick={toggleAllTenants}
              >
                {allSelectable ? "Clear all" : "Select all"}
              </ActionButton>
            </div>
            <ActionButton
              tone="danger"
              className={layout.selectionDelete}
              aria-label="Delete selected Tenants"
              disabled={selectedCount === 0 || mutationBusy}
              onClick={() => requestTenantDelete([...selectedKeys].map((key) => key.slice(8)))}
            >
              <Trash2 size={iconSize.xs} aria-hidden="true" /> Delete
            </ActionButton>
          </>
        ) : (
          <div className={layout.toolbarActions}>
            <RefreshButton
              label="Refresh Tenants"
              busyLabel="Refreshing Tenants"
              busy={refreshing}
              disabled={refreshing || loadingTenants}
              compactOnNarrow
              onClick={() => void refreshTenants()}
            >
              Refresh
            </RefreshButton>
            <IconLabelButton
              className={layout.selectionEnter}
              aria-label="Select Tenants"
              disabled={selectableKeys.length === 0 || refreshing || loadingTenants || busy}
              onClick={enterSelection}
              compactOnNarrow
              icon={<ListChecks size={iconSize.xs} aria-hidden="true" />}
            >
              Select
            </IconLabelButton>
          </div>
        )}
      </div>
      <div className={layout.list} aria-busy={refreshing || loadingTenants}>
        {loadingTenants ? (
          <Loading />
        ) : (
          <div className={layout.rowGroup}>
            {hostTenant && (
              <div
                className={`${layout.row} ${styles.tenantRow} ${inspectedKey === "host" ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
              >
                <button
                  ref={(element) => registerTenantRow("host", element)}
                  type="button"
                  className={styles.configRowMain}
                  aria-label={selectionMode ? "Host Tenant cannot be selected" : "Host Tenant"}
                  aria-pressed={!selectionMode && inspectedKey === "host"}
                  disabled={refreshing || selectionMode}
                  onClick={() => {
                    onLocationChange(tenantLocation("host"));
                  }}
                >
                  <HostTenantIcon size={iconSize.sm} data-icon="host-tenant" />
                  <span className={styles.tenantRowText}>
                    <strong>Host Tenant</strong>
                    <small className={styles.tenantPath} title={hostTenant.home}>
                      {abbreviateTenantHome(hostTenant.home, hostTenant.home)}
                    </small>
                  </span>
                </button>
              </div>
            )}
            <div className={layout.divider}>
              <span>Managed Tenants</span>
              <IconButton
                className={layout.addAction}
                label="Create Managed Tenant"
                disabled={mutationBusy || refreshing || selectionMode}
                onClick={openCreateDialog}
              >
                <Plus size={iconSize.xs} aria-hidden="true" />
              </IconButton>
            </div>
            {managedTenants.map((row) => {
              const key = tenantSelectionValueOf(row);
              const isDefault = row.name === "default";
              const selectedForInspection = key === inspectedKey;
              const selectedForDeletion = selectedKeys.has(key);
              return (
                <div
                  key={key}
                  className={`${layout.row} ${styles.tenantRow} ${selectedForInspection ? layout.rowInspected : ""} ${selectedForDeletion ? layout.rowSelected : ""} ${selectionMode ? layout.rowSelectable : ""} ${isDefault ? layout.rowProtected : ""}`}
                >
                  <button
                    ref={(element) => registerTenantRow(key, element)}
                    type="button"
                    className={styles.configRowMain}
                    aria-label={
                      selectionMode
                        ? isDefault
                          ? "Default Managed Tenant is protected and cannot be selected"
                          : `${selectedForDeletion ? "Deselect" : "Select"} ${row.display_name}`
                        : `${row.display_name}, Managed Tenant`
                    }
                    aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                    disabled={refreshing || (selectionMode && isDefault)}
                    onClick={() => {
                      if (selectionMode) toggleTenant(key);
                      else {
                        onLocationChange(tenantLocation(key));
                      }
                    }}
                  >
                    <ManagedTenantIcon size={iconSize.sm} data-icon="managed-tenant" />
                    <span className={styles.tenantRowText}>
                      <strong>{row.display_name}</strong>
                      <small className={styles.tenantPath} title={row.home}>
                        {abbreviateTenantHome(row.home, hostTenant?.home ?? null)}
                      </small>
                    </span>
                    {selectionMode && !isDefault && (
                      <span className={layout.selectionIndicator} aria-hidden="true">
                        {selectedForDeletion && <Check size={iconSize.xs} strokeWidth={3} />}
                      </span>
                    )}
                  </button>
                  {!selectionMode && !isDefault && (
                    <div className={layout.rowActions}>
                      <IconButton
                        className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                        tone="dangerQuiet"
                        label={`Delete Tenant ${row.display_name}`}
                        disabled={mutationBusy}
                        onClick={() => requestTenantDelete([row.name])}
                      >
                        <Trash2 size={iconSize.xs} />
                      </IconButton>
                    </div>
                  )}
                </div>
              );
            })}
            {managedTenants.length === 0 && !feedback.error && !tenantCatalogError && (
              <EmptyState
                variant="list"
                icon={<ManagedTenantIcon size={iconSize.lg} aria-hidden="true" />}
                title="No Managed Tenants found."
              />
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
