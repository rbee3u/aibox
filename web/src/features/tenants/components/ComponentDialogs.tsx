import { AlertTriangle, Download, LoaderCircle } from "lucide-react";
import {
  canonicalComponentStatus,
  componentLabel,
} from "@/features/tenants/components/componentCatalog";
import type { TenantViewModel } from "@/features/tenants/viewTypes";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { iconSize } from "@/shared/icons/iconSizes";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/components/ComponentDialogs.module.css";

export function ComponentDialogs({
  components,
  dialogs,
  mutations,
}: Pick<TenantViewModel, "components" | "dialogs" | "mutations">) {
  const { submitSpecificVersion } = components;
  const { mutationBusy } = mutations;
  const {
    cancelComponentRemove,
    cancelComponentUpdate,
    changeSpecificVersion,
    closeSpecificVersion,
    componentRemoveTarget,
    componentUpdateTarget,
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
  return (
    <>
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
