import { Download, LoaderCircle, Plus } from "lucide-react";

import { canonicalComponentStatus, componentLabel } from "@/features/tenants/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";

export function TenantDialogs({
  components,
  dialogs,
  mutations,
}: Pick<TenantViewModel, "components" | "dialogs" | "mutations">) {
  const { submitSpecificVersion } = components;
  const {
    cancelComponentRemove,
    componentRemoveTarget,
    changeSpecificVersion,
    closeSpecificVersion,
    cancelDeleteDialog,
    changeNewName,
    closeCreateDialog,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTitleId,
    deleteTarget,
    newName,
    removeComponent,
    specificVersion,
    specificVersionError,
    specificVersionHelpId,
    specificVersionTarget,
    specificVersionTitleId,
    specificVersionValid,
    specificVersionValidationError,
  } = dialogs;
  const { busy, createTenant, deleteTenants, mutationBusy } = mutations;
  return (
    <>
      {createOpen && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={createTitleId}
          busy={mutationBusy}
          onCancel={closeCreateDialog}
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
                onChange={(event) => changeNewName(event.target.value)}
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
              <button type="button" onClick={closeCreateDialog} disabled={busy}>
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
          onCancel={cancelDeleteDialog}
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
          onCancel={cancelDeleteDialog}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {specificVersionTarget && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={specificVersionTitleId}
          busy={mutationBusy}
          onCancel={closeSpecificVersion}
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
                onChange={(event) => changeSpecificVersion(event.target.value)}
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
              <button type="button" onClick={closeSpecificVersion} disabled={mutationBusy}>
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
          onCancel={cancelComponentRemove}
          onConfirm={() => void removeComponent()}
        />
      )}
    </>
  );
}
