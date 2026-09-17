import { AlertTriangle, Download, LoaderCircle } from "lucide-react";
import { useRef, useState } from "react";

import { canonicalComponentStatus, componentLabel } from "@/features/tenants/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/useTenantController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";
import { resourceIcons } from "@/shared/icons/consoleIcons";

const ManagedTenantIcon = resourceIcons.managedTenant;

export function TenantDialogs({
  catalog,
  components,
  dialogs,
  mutations,
}: Pick<TenantViewModel, "components" | "dialogs" | "mutations"> & {
  catalog?: TenantViewModel["catalog"];
}) {
  const { submitSpecificVersion } = components;
  const {
    cancelComponentRemove,
    cancelComponentUpdate,
    componentRemoveTarget,
    componentUpdateTarget,
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
    updateComponent,
  } = dialogs;
  const { busy, createTenant, deleteTenants, mutationBusy } = mutations;
  return (
    <>
      {createOpen && (
        <CreateTenantDialog
          busy={busy}
          changeNewName={changeNewName}
          closeCreateDialog={closeCreateDialog}
          createError={createError}
          createHelpId={createHelpId}
          createNameTaken={createNameTaken}
          createNameValid={createNameValid}
          createTenant={createTenant}
          createTitleId={createTitleId}
          mutationBusy={mutationBusy}
          newName={newName}
        />
      )}
      {deleteTarget?.names.length === 1 &&
        (() => {
          const targetName = deleteTarget.names[0];
          const targetTenant = catalog?.managedTenants.find((t) => t.name === targetName);
          const homePath = targetTenant
            ? abbreviateTenantHome(targetTenant.home, catalog?.hostTenant?.home ?? null)
            : `~/.aibox/tenants/${targetName}`;
          return (
            <ConfirmDialog
              title={`Delete Tenant ${targetName}?`}
              facts={[
                { label: "Target", value: <code>{targetName}</code> },
                { label: "Type", value: "Managed Tenant" },
                {
                  label: "Tenant Home",
                  value: <code title={targetTenant?.home ?? undefined}>{homePath}</code>,
                  fullWidth: true,
                },
              ]}
              message="Permanently deletes Tenant Home, Sessions, Components state, and Named Configs."
              confirmation={targetName}
              confirmLabel="Delete"
              busy={mutationBusy}
              onCancel={cancelDeleteDialog}
              onConfirm={() => void deleteTenants()}
            />
          );
        })()}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Managed Tenants?"
          message="Permanently deletes each Tenant Home, Sessions, Components state, and Named Configs."
          description={
            <div className={styles.batchDeletePlan}>
              <div className={styles.batchDeleteCount}>
                <strong>{deleteTarget.names.length}</strong> Managed Tenants will be permanently
                removed:
              </div>
              <div className={layout.planList}>
                {deleteTarget.names.map((name) => (
                  <div key={name} className={styles.batchDeleteRow}>
                    <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                    <code>{name}</code>
                  </div>
                ))}
              </div>
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
                icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
              >
                {specificVersionValidationError}
              </AlertBanner>
            )}
            {specificVersionError && (
              <AlertBanner
                className={layout.alertBanner}
                tone="danger"
                icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
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
                  <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
                ) : (
                  <Download size={iconSize.xs} />
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
      {componentUpdateTarget && (
        <ConfirmDialog
          title={`Update ${componentLabel(componentUpdateTarget.row.kind)}?`}
          facts={[
            { label: "Tenant", value: componentUpdateTarget.tenantLabel },
            {
              label: "Current state",
              value: canonicalComponentStatus(componentUpdateTarget.row),
            },
          ]}
          message="Rewrites the statusline files to the current AIBox definition. Edits made by hand are lost."
          confirmLabel="Update"
          busyLabel="Updating…"
          variant="primary"
          busy={mutationBusy}
          onCancel={cancelComponentUpdate}
          onConfirm={() => void updateComponent()}
        />
      )}
    </>
  );
}

type CreateTenantDialogProps = Pick<
  TenantViewModel["dialogs"],
  | "changeNewName"
  | "closeCreateDialog"
  | "createError"
  | "createHelpId"
  | "createNameTaken"
  | "createNameValid"
  | "createTitleId"
  | "newName"
> &
  Pick<TenantViewModel["mutations"], "busy" | "createTenant" | "mutationBusy">;

/**
 * The format rule is already under the field, so its error waits until the
 * user leaves the field or submits — a name like `my-` is invalid only for the
 * keystroke it takes to finish it. A taken name is reported as soon as it
 * matches, since that never flickers.
 */
function CreateTenantDialog({
  busy,
  changeNewName,
  closeCreateDialog,
  createError,
  createHelpId,
  createNameTaken,
  createNameValid,
  createTenant,
  createTitleId,
  mutationBusy,
  newName,
}: CreateTenantDialogProps) {
  const cancelButtonRef = useRef<HTMLButtonElement>(null);
  const [nameTouched, setNameTouched] = useState(false);
  return (
    <Dialog
      className={layout.dialog}
      ariaLabelledBy={createTitleId}
      busy={mutationBusy}
      onCancel={closeCreateDialog}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setNameTouched(true);
          if (createNameValid && !createNameTaken && !mutationBusy) void createTenant();
        }}
      >
        <div className={styles.dialogHeader}>
          <div className={styles.dialogIconContainer}>
            <ManagedTenantIcon size={iconSize.md} aria-hidden="true" />
          </div>
          <div>
            <h2 id={createTitleId} className={styles.dialogTitle}>
              Create Managed Tenant
            </h2>
            <p className={styles.dialogSubtitle}>
              Provision an isolated filesystem sandbox environment for Coding Agents.
            </p>
          </div>
        </div>
        <div className={styles.dialogField}>
          <label htmlFor="create-tenant-name" className={styles.fieldLabel}>
            Tenant Name
          </label>
          <TextInput
            id="create-tenant-name"
            autoFocus
            aria-label="Tenant name"
            placeholder="e.g. project-dev, task-sandbox"
            value={newName}
            onChange={(event) => changeNewName(event.target.value)}
            onBlur={(event) => {
              if (event.relatedTarget !== cancelButtonRef.current && newName.trim().length > 0) {
                setNameTouched(true);
              }
            }}
            onKeyDown={(event) => {
              // A disabled Create blocks implicit submission, so Enter reveals why.
              if (event.key === "Enter") setNameTouched(true);
            }}
            aria-invalid={(nameTouched && !createNameValid) || createNameTaken}
            aria-describedby={createHelpId}
          />
          <div className={styles.pathPreview}>
            <span className={styles.pathPreviewLabel}>Filesystem Sandbox:</span>
            <code>~/.aibox/tenants/{newName.trim() || "<name>"}</code>
          </div>
        </div>
        <p id={createHelpId} className={layout.dialogDescription}>
          Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or number.
        </p>
        {nameTouched && !createNameValid && (
          <AlertBanner
            className={layout.alertBanner}
            tone="danger"
            icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
          >
            Enter a valid lowercase DNS label.
          </AlertBanner>
        )}
        {createNameValid && createNameTaken && (
          <AlertBanner
            className={layout.alertBanner}
            tone="danger"
            icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
          >
            Managed Tenant {newName} already exists.
          </AlertBanner>
        )}
        {createError && (
          <AlertBanner
            className={layout.alertBanner}
            tone="danger"
            icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
          >
            {createError}
          </AlertBanner>
        )}
        <div className={styles.dialogActions}>
          <ActionButton
            ref={cancelButtonRef}
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
                <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
                Creating…
              </>
            ) : (
              "Create"
            )}
          </ActionButton>
        </div>
      </form>
    </Dialog>
  );
}
