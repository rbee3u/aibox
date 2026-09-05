import { AlertTriangle, Download, LoaderCircle } from "lucide-react";

import { canonicalComponentStatus, componentLabel } from "@/features/tenants/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
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
    createNameTaken,
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
              if (createNameValid && !createNameTaken && !mutationBusy) void createTenant();
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
                aria-invalid={newName.length > 0 && (!createNameValid || createNameTaken)}
                aria-describedby={createHelpId}
              />
            </label>
            <p id={createHelpId} className={layout.dialogDescription}>
              Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or
              number.
            </p>
            {newName.length > 0 && !createNameValid && (
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                Enter a valid lowercase DNS label.
              </AlertBanner>
            )}
            {createNameValid && createNameTaken && (
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                Managed Tenant {newName} already exists.
              </AlertBanner>
            )}
            {createError && (
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                {createError}
              </AlertBanner>
            )}
            <div className={styles.dialogActions}>
              <ActionButton
                type="button"
                tone="secondary"
                onClick={closeCreateDialog}
                disabled={busy}
              >
                Cancel
              </ActionButton>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!createNameValid || createNameTaken || mutationBusy}
              >
                {busy ? (
                  <>
                    <LoaderCircle className="spin" size={14} aria-hidden="true" />
                    Creating…
                  </>
                ) : (
                  "Create"
                )}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.names[0]}?`}
          message="Permanently deletes Tenant Home, Sessions, Components state, and Named Configs."
          confirmation={deleteTarget.names[0]}
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDeleteDialog}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Managed Tenants?"
          message="Permanently deletes each Tenant Home, Sessions, Components state, and Named Configs."
          description={
            <div className={layout.planList}>
              {deleteTarget.names.map((name) => (
                <code key={name}>{name}</code>
              ))}
            </div>
          }
          confirmLabel="Delete"
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
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                {specificVersionValidationError}
              </AlertBanner>
            )}
            {specificVersionError && (
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                {specificVersionError}
              </AlertBanner>
            )}
            <div className={styles.dialogActions}>
              <ActionButton
                type="button"
                tone="secondary"
                onClick={closeSpecificVersion}
                disabled={mutationBusy}
              >
                Cancel
              </ActionButton>
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
          facts={[
            { label: "Tenant", value: componentRemoveTarget.tenantLabel },
            {
              label: "Current state",
              value: canonicalComponentStatus(componentRemoveTarget.row),
            },
          ]}
          message="Deletes Component-owned state. Workspace environments and user-owned package, cache, credential, and config state are kept."
          confirmLabel="Remove"
          busyLabel="Removing…"
          busy={mutationBusy}
          onCancel={cancelComponentRemove}
          onConfirm={() => void removeComponent()}
        />
      )}
    </>
  );
}
