import { Check, ListChecks, Plus, Trash2 } from "lucide-react";

import { abbreviateTenantHome } from "@/features/tenants/componentCatalog";
import { tenantLocation, tenantSelectionValueOf } from "@/features/tenants/route";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { EmptyState } from "@/shared/ui/EmptyState";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";

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
  const { selectedKey } = detail;
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
            <button
              type="button"
              className={layout.selectionCancel}
              disabled={busy}
              onClick={cancelSelection}
            >
              Cancel
            </button>
            <div className={layout.selectionCenter}>
              <span className={layout.selectionCount}>{selectedCount} selected</span>
              <button
                type="button"
                className={layout.selectionAll}
                disabled={selectableKeys.length === 0 || busy}
                onClick={toggleAllTenants}
              >
                {allSelectable ? "Clear all" : "Select all"}
              </button>
            </div>
            <button
              type="button"
              className={layout.selectionDelete}
              aria-label="Delete selected Tenants"
              disabled={selectedCount === 0 || mutationBusy}
              onClick={() => requestTenantDelete([...selectedKeys].map((key) => key.slice(8)))}
            >
              <Trash2 size={14} aria-hidden="true" /> Delete
            </button>
          </>
        ) : (
          <div className={layout.toolbarActions}>
            <RefreshButton
              className={layout.refreshAction}
              label="Refresh Tenants"
              busyLabel="Refreshing Tenants"
              busy={refreshing}
              disabled={refreshing || loadingTenants}
              onClick={() => void refreshTenants()}
            >
              Refresh
            </RefreshButton>
            <button
              type="button"
              className={layout.selectionEnter}
              aria-label="Select Tenants"
              disabled={selectableKeys.length === 0 || refreshing || loadingTenants || busy}
              onClick={enterSelection}
            >
              <ListChecks size={14} /> Select
            </button>
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
                className={`${layout.row} ${styles.tenantRow} ${selectedKey === "host" ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
              >
                <button
                  ref={(element) => registerTenantRow("host", element)}
                  type="button"
                  className={styles.configRowMain}
                  aria-label={selectionMode ? "Host Tenant cannot be selected" : "Host Tenant"}
                  aria-pressed={!selectionMode && selectedKey === "host"}
                  disabled={refreshing || selectionMode}
                  onClick={() => {
                    onLocationChange(tenantLocation("host"));
                  }}
                >
                  <HostTenantIcon size={16} data-icon="host-tenant" />
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
              <ActionButton
                className={layout.addAction}
                aria-label="Create Managed Tenant"
                disabled={mutationBusy || refreshing || selectionMode}
                onClick={openCreateDialog}
              >
                <Plus size={15} aria-hidden="true" />
                Create
              </ActionButton>
            </div>
            {managedTenants.map((row) => {
              const key = tenantSelectionValueOf(row);
              const isDefault = row.name === "default";
              const selectedForInspection = key === selectedKey;
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
                    <ManagedTenantIcon size={16} data-icon="managed-tenant" />
                    <span className={styles.tenantRowText}>
                      <strong>{row.display_name}</strong>
                      <small className={styles.tenantPath} title={row.home}>
                        {abbreviateTenantHome(row.home, hostTenant?.home ?? null)}
                      </small>
                    </span>
                    {selectionMode && !isDefault && (
                      <span className={layout.selectionIndicator} aria-hidden="true">
                        {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                      </span>
                    )}
                  </button>
                  {!selectionMode && !isDefault && (
                    <div className={layout.rowActions}>
                      <IconButton
                        className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                        tone="danger"
                        label={`Delete Tenant ${row.display_name}`}
                        disabled={mutationBusy}
                        onClick={() => requestTenantDelete([row.name])}
                      >
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  )}
                </div>
              );
            })}
            {managedTenants.length === 0 && !feedback.error && !tenantCatalogError && (
              <EmptyState
                variant="list"
                icon={<ManagedTenantIcon size={22} aria-hidden="true" />}
                title="No Managed Tenants found."
              />
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
