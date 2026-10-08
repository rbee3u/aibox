import styles from "@/features/tenants/mutation/TenantDialogs.module.css";
import { AlertTriangle, LoaderCircle } from "lucide-react";
import { useRef, useState } from "react";

import type { TenantViewModel } from "@/features/tenants/viewTypes";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import layout from "@/shared/ui/layout/catalog.module.css";

import { iconSize } from "@/shared/icons/iconSizes";
import { resourceIcons } from "@/shared/icons/consoleIcons";

const ManagedTenantIcon = resourceIcons.managedTenant;

export function TenantDialogs({
  dialogs,
  mutations,
}: Pick<TenantViewModel, "dialogs" | "mutations">) {
  const {
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
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.names[0]}?`}
          message="Permanently removes this tenant and its filesystem sandbox. This action cannot be undone."
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDeleteDialog}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title={`Delete ${deleteTarget.names.length} selected Tenants?`}
          message="Permanently removes the selected tenants and all associated data. This action cannot be undone."
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDeleteDialog}
          onConfirm={() => void deleteTenants()}
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
 * Delay format errors until blur or submit to avoid flagging unfinished names.
 * Report existing names immediately.
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
              Provision an isolated filesystem sandbox environment for Agents.
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
