import {
  Check,
  ChevronLeft,
  Clipboard,
  Download,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";

import type { Operation } from "@/api/operations";
import type { TenantApi } from "@/api/tenants";
import { ComponentCatalogSkeleton } from "@/features/tenants/components/ComponentCatalogSkeleton";
import { ComponentRowItem } from "@/features/tenants/components/ComponentRowItem";
import {
  abbreviateTenantHome,
  canonicalComponentStatus,
  componentLabel,
  componentRowModel,
  relativeTimeLabel,
} from "@/features/tenants/componentCatalog";
import { tenantKeyOf, tenantLocation } from "@/features/tenants/route";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { useTenantController } from "@/features/tenants/useTenantController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { TextInput } from "@/shared/ui/FormControls";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

interface PageProps {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export function TenantPage(props: PageProps) {
  const {
    allSelectable,
    attentionComponentCount,
    busy,
    cancelSelection,
    checkingLatest,
    checkForUpdates,
    componentActionProgress,
    componentCatalogLoading,
    componentGroups,
    componentMenuButtons,
    componentMenuItems,
    componentMenuPosition,
    componentMenuRef,
    componentRemoveTarget,
    componentTotalCount,
    copiedHome,
    copyHome,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTenant,
    createTitleId,
    deleteTarget,
    deleteTenants,
    detailHeadingRef,
    detailOpen,
    error,
    expandedComponents,
    hostTenant,
    installedComponentCount,
    latestSnapshot,
    loadComponents,
    loadingTenants,
    managedTenants,
    mutateComponent,
    mutationBusy,
    newName,
    openComponentMenu,
    openSpecificVersion,
    refreshing,
    refreshTenants,
    requestTenantDelete,
    retryTenantPage,
    selected,
    selectedCount,
    selectedHome,
    selectedKey,
    selectedKeys,
    selectableKeys,
    selectionMode,
    setComponentMenuPosition,
    setComponentRemoveTarget,
    setCreateError,
    setCreateOpen,
    setDeleteTarget,
    setDetailOpen,
    setExpandedComponents,
    setNewName,
    setOpenComponentMenu,
    setSelectedKey,
    setSelectionMode,
    setSpecificVersion,
    setSpecificVersionError,
    setSpecificVersionTarget,
    specificVersion,
    specificVersionError,
    specificVersionHelpId,
    specificVersionTarget,
    specificVersionTitleId,
    specificVersionValid,
    specificVersionValidationError,
    submitSpecificVersion,
    tenantCatalogError,
    tenantKindLabel,
    tenantRowButtons,
    toggleAllTenants,
    toggleTenant,
  } = useTenantController(props);
  const { onLocationChange, operation } = props;
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError error={error ?? tenantCatalogError} onRetry={() => void retryTenantPage()} />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${detailOpen ? layout.showsDetail : ""}`}>
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
                  onClick={() => setSelectionMode(true)}
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
                      ref={(element) => {
                        if (element) tenantRowButtons.current.set("host", element);
                        else tenantRowButtons.current.delete("host");
                      }}
                      type="button"
                      className={styles.configRowMain}
                      aria-label={selectionMode ? "Host Tenant cannot be selected" : "Host Tenant"}
                      aria-pressed={!selectionMode && selectedKey === "host"}
                      disabled={refreshing || selectionMode}
                      onClick={() => {
                        setSelectedKey("host");
                        setDetailOpen(true);
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
                  <IconButton
                    className={layout.addAction}
                    label="Create Managed Tenant"
                    disabled={mutationBusy || refreshing || selectionMode}
                    onClick={() => {
                      setCreateError(null);
                      setCreateOpen(true);
                    }}
                  >
                    <Plus size={15} />
                  </IconButton>
                </div>
                {managedTenants.map((row) => {
                  const key = tenantKeyOf(row);
                  const isDefault = row.name === "default";
                  const selectedForInspection = key === selectedKey;
                  const selectedForDeletion = selectedKeys.has(key);
                  return (
                    <div
                      key={key}
                      className={`${layout.row} ${styles.tenantRow} ${selectedForInspection ? layout.rowInspected : ""} ${selectedForDeletion ? layout.rowSelected : ""} ${selectionMode ? layout.rowSelectable : ""} ${isDefault ? layout.rowProtected : ""}`}
                    >
                      <button
                        ref={(element) => {
                          if (element) tenantRowButtons.current.set(key, element);
                          else tenantRowButtons.current.delete(key);
                        }}
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
                            setSelectedKey(key);
                            setDetailOpen(true);
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
                {managedTenants.length === 0 && !error && !tenantCatalogError && (
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
                      setSelectedKey(null);
                      setDetailOpen(false);
                      onLocationChange(new URLSearchParams());
                      window.requestAnimationFrame(() => {
                        if (focusKey) tenantRowButtons.current.get(focusKey)?.focus();
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
                          {copiedHome === selected.home ? (
                            <Check size={13} />
                          ) : (
                            <Clipboard size={13} />
                          )}
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
                    <IconButton
                      className={styles.componentCheckButton}
                      label={checkingLatest ? "Checking for updates" : "Check for updates"}
                      aria-busy={checkingLatest || undefined}
                      disabled={checkingLatest}
                      onClick={() => void checkForUpdates()}
                    >
                      <RefreshCw
                        className={checkingLatest ? "spin" : undefined}
                        size={15}
                        aria-hidden="true"
                      />
                    </IconButton>
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
                                componentActionProgress?.tenantKey === selectedKey &&
                                componentActionProgress.kind === row.kind
                                  ? componentActionProgress.label
                                  : null;
                              return (
                                <ComponentRowItem
                                  key={row.kind}
                                  row={row}
                                  model={model}
                                  expanded={expandedComponents.has(row.kind)}
                                  progressLabel={rowProgress}
                                  busy={busy}
                                  mutationBusy={mutationBusy}
                                  openMenu={openComponentMenu}
                                  menuPosition={componentMenuPosition}
                                  menuRef={componentMenuRef}
                                  onToggleExpanded={() =>
                                    setExpandedComponents((current) => {
                                      const next = new Set(current);
                                      if (next.has(row.kind)) next.delete(row.kind);
                                      else next.add(row.kind);
                                      return next;
                                    })
                                  }
                                  onRetryInspection={() => void loadComponents(selected)}
                                  onInstall={() => void mutateComponent(row, true)}
                                  onRemove={() =>
                                    setComponentRemoveTarget({
                                      row,
                                      tenantLabel: selected.display_name,
                                    })
                                  }
                                  onOpenSpecificVersion={() =>
                                    openSpecificVersion(row, model.specificVersionMode)
                                  }
                                  onMenuPosition={setComponentMenuPosition}
                                  onOpenMenu={setOpenComponentMenu}
                                  registerMenuButton={(element) => {
                                    if (element)
                                      componentMenuButtons.current.set(row.kind, element);
                                    else componentMenuButtons.current.delete(row.kind);
                                  }}
                                  registerMenuItem={(element) => {
                                    if (element) componentMenuItems.current.set(row.kind, element);
                                    else componentMenuItems.current.delete(row.kind);
                                  }}
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
      </div>
      {createOpen && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={createTitleId}
          busy={mutationBusy}
          onCancel={() => setCreateOpen(false)}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (createNameValid && !mutationBusy) void createTenant();
            }}
          >
            <h2 id={createTitleId}>Create Managed Tenant</h2>
            <label>
              Name
              <TextInput
                autoFocus
                aria-label="Tenant name"
                value={newName}
                onChange={(event) => {
                  setNewName(event.target.value);
                  setCreateError(null);
                }}
                aria-invalid={newName.length > 0 && !createNameValid}
                aria-describedby={createHelpId}
              />
            </label>
            <p id={createHelpId} className={layout.dialogDescription}>
              Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or
              number.
            </p>
            {newName.length > 0 && !createNameValid && (
              <div className={layout.alertBanner} role="alert">
                Enter a valid lowercase DNS label.
              </div>
            )}
            {createError && <div className={layout.alertBanner}>{createError}</div>}
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setCreateOpen(false)} disabled={busy}>
                Cancel
              </button>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!createNameValid || mutationBusy}
              >
                {busy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Plus size={14} />
                )}
                {busy ? "Creating…" : "Create"}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.names[0]}?`}
          description={
            <p className={layout.dialogDescription}>
              This permanently deletes the Tenant Home, Sessions, Components state, and Named
              Configs for this Tenant.
            </p>
          }
          confirmation={deleteTarget.names[0]}
          confirmLabel="Delete Tenant"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Managed Tenants?"
          description={
            <>
              <p className={layout.dialogDescription}>
                This permanently deletes each Tenant Home, its Sessions and Components state, and
                its Named Configs.
              </p>
              <div className={layout.planList}>
                {deleteTarget.names.map((name) => (
                  <code key={name}>{name}</code>
                ))}
              </div>
            </>
          }
          confirmLabel="Delete selected"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {specificVersionTarget && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={specificVersionTitleId}
          busy={mutationBusy}
          onCancel={() => setSpecificVersionTarget(null)}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (specificVersionValid && !mutationBusy) void submitSpecificVersion();
            }}
          >
            <h2 id={specificVersionTitleId}>
              {specificVersionTarget.mode === "update"
                ? `Update ${componentLabel(specificVersionTarget.row.kind)} version`
                : `Install ${componentLabel(specificVersionTarget.row.kind)} version`}
            </h2>
            <p className={layout.dialogDescription}>
              Tenant: <strong>{specificVersionTarget.tenantLabel}</strong>
            </p>
            <label>
              Version
              <TextInput
                autoFocus
                aria-label="Component version"
                value={specificVersion}
                placeholder="X.Y.Z"
                onChange={(event) => {
                  setSpecificVersion(event.target.value);
                  setSpecificVersionError(null);
                }}
                aria-invalid={Boolean(specificVersionValidationError)}
                aria-describedby={specificVersionHelpId}
              />
            </label>
            <p id={specificVersionHelpId} className={layout.dialogDescription}>
              {specificVersionTarget.mode === "update"
                ? `Enter a stable version newer than v${specificVersionTarget.row.version}.`
                : "Enter a stable version in X.Y.Z form."}
            </p>
            {specificVersionValidationError && (
              <div className={layout.alertBanner} role="alert">
                {specificVersionValidationError}
              </div>
            )}
            {specificVersionError && (
              <div className={layout.alertBanner} role="alert">
                {specificVersionError}
              </div>
            )}
            <div className={styles.dialogActions}>
              <button
                type="button"
                onClick={() => setSpecificVersionTarget(null)}
                disabled={mutationBusy}
              >
                Cancel
              </button>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!specificVersionValid || mutationBusy}
              >
                {mutationBusy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Download size={14} />
                )}
                {mutationBusy
                  ? specificVersionTarget.mode === "update"
                    ? "Updating…"
                    : "Installing…"
                  : specificVersionTarget.mode === "update"
                    ? "Update version"
                    : "Install version"}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {componentRemoveTarget && (
        <ConfirmDialog
          title={`Remove ${componentLabel(componentRemoveTarget.row.kind)}?`}
          description={
            <div className={layout.dialogDescription}>
              <p>
                Tenant: <strong>{componentRemoveTarget.tenantLabel}</strong>
              </p>
              <p>
                Current state:{" "}
                <strong>{canonicalComponentStatus(componentRemoveTarget.row)}</strong>
              </p>
              <p>
                Existing Component-owned state will be deleted. Workspace environments and
                user-owned package, cache, credential, and configuration state are preserved.
              </p>
            </div>
          }
          confirmLabel="Remove Component"
          busy={mutationBusy}
          onCancel={() => setComponentRemoveTarget(null)}
          onConfirm={() => {
            const row = componentRemoveTarget.row;
            void mutateComponent(row, false).then(() => setComponentRemoveTarget(null));
          }}
        />
      )}
    </div>
  );
}
