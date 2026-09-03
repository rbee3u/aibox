import { AlertTriangle, LoaderCircle } from "lucide-react";

import { propagationDetail, propagationGroup } from "@/features/configs/configCatalog";
import type { ConfigViewModel } from "@/features/configs/useConfigController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";

export function ConfigDialogs({
  catalog,
  dialogs,
  editor,
  mutations,
}: Pick<ConfigViewModel, "catalog" | "dialogs" | "editor" | "mutations">) {
  const { agent, configTenantLabel, fileStatuses, tenant } = catalog;
  const {
    applyTarget,
    cancelApply,
    cancelDelete,
    cancelPending,
    changeNewName,
    closeCreateDialog,
    closePropagation,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTitleId,
    deleteTarget,
    discardAndRunPendingAction,
    newName,
    pendingAction,
    preview,
    propagationHasFailures,
    propagationNeedsAttention,
    propagationTitleId,
    report,
    unsavedTitleId,
  } = dialogs;
  const { dirtyFiles } = editor;
  const {
    applyConfig,
    busy,
    createConfig,
    deleteConfigs,
    executePropagation,
    mutationBusy,
    saveOrder,
    savePending,
  } = mutations;
  return (
    <>
      {pendingAction && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={unsavedTitleId}
          busy={mutationBusy}
          onCancel={cancelPending}
        >
          <section>
            <h2 id={unsavedTitleId}>Unsaved changes</h2>
            <p>
              Save changes to{" "}
              {dirtyFiles.length > 1
                ? `${dirtyFiles.length} files`
                : (dirtyFiles[0] ?? "this file")}{" "}
              before continuing?
            </p>
            <div className={styles.dialogActions}>
              <ActionButton type="button" tone="secondary" onClick={cancelPending} disabled={busy}>
                Cancel
              </ActionButton>
              <ActionButton
                type="button"
                tone="secondary"
                onClick={() => void discardAndRunPendingAction()}
                disabled={busy}
              >
                Discard and continue
              </ActionButton>
              <ActionButton
                tone="primarySoft"
                onClick={() => void savePending(saveOrder)}
                disabled={mutationBusy || dirtyFiles.some((name) => !fileStatuses[name]?.canSave)}
              >
                Save and continue
              </ActionButton>
            </div>
          </section>
        </Dialog>
      )}
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
              if (createNameValid && !mutationBusy) void createConfig(newName);
            }}
          >
            <h2 id={createTitleId}>Create Named Config</h2>
            <label>
              Name
              <TextInput
                autoFocus
                aria-label="Named Config name"
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
              <AlertBanner
                className={styles.dialogAlert}
                tone="danger"
                icon={<AlertTriangle size={15} aria-hidden="true" />}
              >
                Enter a valid lowercase DNS label.
              </AlertBanner>
            )}
            {createError && (
              <AlertBanner
                className={styles.dialogAlert}
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
                tone="primarySoft"
                disabled={!createNameValid || mutationBusy}
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
      {applyTarget && (
        <ConfirmDialog
          title={`Apply ${applyTarget.name} to Current Config?`}
          facts={[
            { label: "Tenant", value: configTenantLabel },
            { label: "Coding Agent", value: agent === "codex" ? "Codex" : "Claude" },
            { label: "Source", value: `Named Config ${applyTarget.name}` },
            { label: "Target", value: "Current Config" },
          ]}
          message="Present fields replace; omitted fixed fields are removed. Unrelated native config is kept. One-shot; no rollback."
          confirmation={tenant.kind === "host" ? "Host Tenant" : undefined}
          confirmLabel="Apply"
          variant="primary"
          busy={mutationBusy}
          onCancel={cancelApply}
          onConfirm={() => void applyConfig(applyTarget.name)}
        />
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Named Config ${deleteTarget.names[0]}?`}
          message="Deletes this Named Config only. Current Config is unchanged; Drift may become Source missing."
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDelete}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Named Configs?"
          message="Deletes the selected Named Configs only. Current Config is unchanged; Drift may become Source missing."
          description={
            <div className={styles.planList}>
              {deleteTarget.names.map((name) => (
                <code key={name}>{name}</code>
              ))}
            </div>
          }
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDelete}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {(preview || report) && (
        <Dialog
          className={`${layout.dialog} ${styles.wideDialog}`}
          ariaLabelledBy={propagationTitleId}
          busy={mutationBusy}
          onCancel={closePropagation}
        >
          <section>
            <h2 id={propagationTitleId}>
              {preview ? "Credential Propagation preview" : "Credential Propagation result"}
            </h2>
            {report && (
              <div
                className={`${styles.propagationSummary} ${
                  propagationHasFailures || propagationNeedsAttention
                    ? styles.propagationSummaryPartial
                    : styles.propagationSummaryComplete
                }`}
                role={propagationHasFailures ? "alert" : "status"}
              >
                {propagationHasFailures
                  ? "Partially completed. Successful credential updates were kept; failed targets need attention."
                  : propagationNeedsAttention
                    ? "Credential propagation completed with targets that need attention."
                    : "Credential propagation completed."}
              </div>
            )}
            <div className={styles.propagationGroups}>
              {(["updated", "skipped", "attention"] as const).map((group) => {
                const entries = (preview?.preview.entries ?? report?.entries ?? []).filter(
                  (entry) => propagationGroup(entry.outcome.status) === group,
                );
                if (entries.length === 0) return null;
                const heading =
                  group === "updated"
                    ? "Updated"
                    : group === "skipped"
                      ? "Skipped"
                      : "Needs attention";
                return (
                  <section key={group}>
                    <h3>
                      {heading} <span>{entries.length}</span>
                    </h3>
                    <div className={styles.planList}>
                      {entries.map((entry) => (
                        <div key={entry.label}>
                          <code>{entry.label}</code>
                          <span>
                            {preview && entry.outcome.status === "updated"
                              ? "Will update"
                              : entry.outcome.status}
                            {propagationDetail(entry.outcome) && (
                              <small>{propagationDetail(entry.outcome)}</small>
                            )}
                          </span>
                        </div>
                      ))}
                    </div>
                  </section>
                );
              })}
              {(preview?.preview.entries.length ?? report?.entries.length ?? 0) === 0 && (
                <p>No matching credentials.</p>
              )}
            </div>
            <div className={styles.dialogActions}>
              <ActionButton type="button" tone="secondary" onClick={closePropagation}>
                Close
              </ActionButton>
              {preview && (
                <ActionButton
                  tone="primarySoft"
                  disabled={mutationBusy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  {busy && <LoaderCircle className="spin" size={14} aria-hidden="true" />}
                  {busy
                    ? "Propagating…"
                    : `Propagate ${preview.preview.updates} credential update${preview.preview.updates === 1 ? "" : "s"}`}
                </ActionButton>
              )}
            </div>
          </section>
        </Dialog>
      )}
    </>
  );
}
