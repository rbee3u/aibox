import { AlertTriangle, ArrowRight, Check, LoaderCircle } from "lucide-react";

import {
  propagationDetail,
  propagationGroup,
  propagationGroupHeading,
  propagationGroups,
  propagationStatus,
} from "@/features/configs/configCatalog";
import type { ConfigPendingAction } from "@/features/configs/route";
import type { ConfigViewModel } from "@/features/configs/useConfigController";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog, ContextPill } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";

const NamedConfigIcon = resourceIcons.namedConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

/**
 * A mode switch keeps the reader on the file, so it must not read as leaving:
 * the edits live only in the editor being switched away from.
 */
function pendingActionCopy(action: ConfigPendingAction, dirtyFiles: readonly string[]) {
  const subject =
    dirtyFiles.length > 1 ? `${dirtyFiles.length} files` : (dirtyFiles[0] ?? "this file");
  if (action.kind === "leave") {
    return {
      title: "Unsaved changes",
      body: `Save changes to ${subject} before continuing?`,
      verb: "continue",
    };
  }
  const target = action.kind.switchTo === "raw" ? "Raw" : "Visual";
  const source = action.kind.switchTo === "raw" ? "Visual" : "Raw";
  return {
    title: `Switch to ${target}?`,
    body: `Your unsaved edits to ${subject} only exist in the ${source} editor. Save them first, or discard them to switch.`,
    verb: "switch",
  };
}

interface ApplyConfigDescriptionProps {
  tenantLabel: string;
  isHost: boolean;
  agentLabel: string;
  sourceName: string;
}

function ApplyConfigDescription({
  tenantLabel,
  isHost,
  agentLabel,
  sourceName,
}: ApplyConfigDescriptionProps) {
  return (
    <div className={styles.applyDetails}>
      <dl className={styles.applyFlowCard}>
        <div className={styles.applyFlowMeta}>
          <div className={styles.applyMetaGroup}>
            <dt>Tenant</dt>
            <dd>
              <span className={isHost ? styles.applyHostTag : styles.applyTenantTag}>
                {tenantLabel}
              </span>
            </dd>
          </div>
          <div className={styles.applyMetaGroup}>
            <dt>Agent</dt>
            <dd>
              <span className={styles.applyAgentTag}>{agentLabel}</span>
            </dd>
          </div>
        </div>

        <div className={styles.applyFlowTrack}>
          <div className={styles.applyFlowStep}>
            <dt>Source</dt>
            <dd className={styles.applyFlowValue}>Named Config {sourceName}</dd>
          </div>
          <div className={styles.applyFlowArrow} aria-hidden="true">
            <ArrowRight size={iconSize.xs} />
          </div>
          <div className={styles.applyFlowStep}>
            <dt>Target</dt>
            <dd className={styles.applyFlowValue}>Current Config</dd>
          </div>
        </div>
      </dl>

      <ul className={styles.applyRulesList}>
        <li>
          <Check size={iconSize.xs} className={styles.applyRuleCheck} aria-hidden="true" />
          <span>Replaces declared fields and removes omitted fixed fields</span>
        </li>
        <li>
          <Check size={iconSize.xs} className={styles.applyRuleCheck} aria-hidden="true" />
          <span>Preserves unrelated native configuration</span>
        </li>
      </ul>

      {isHost && (
        <AlertBanner
          className={styles.applyHostAlert}
          tone="warning"
          icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
        >
          Writes directly to Host Home — one-shot with no rollback.
        </AlertBanner>
      )}
    </div>
  );
}

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
    createNameTaken,
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
  const pendingCopy = pendingAction && pendingActionCopy(pendingAction, dirtyFiles);
  return (
    <>
      {pendingAction && pendingCopy && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={unsavedTitleId}
          busy={mutationBusy}
          onCancel={cancelPending}
        >
          <section>
            <h2 id={unsavedTitleId}>{pendingCopy.title}</h2>
            <p>{pendingCopy.body}</p>
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
                Discard and {pendingCopy.verb}
              </ActionButton>
              <ActionButton
                tone="primary"
                onClick={() => void savePending(saveOrder)}
                disabled={mutationBusy || dirtyFiles.some((name) => !fileStatuses[name]?.canSave)}
              >
                Save and {pendingCopy.verb}
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
              if (createNameValid && !createNameTaken && !mutationBusy) void createConfig(newName);
            }}
          >
            <div className={styles.dialogHeader}>
              <div className={styles.dialogIconContainer}>
                <NamedConfigIcon size={iconSize.md} aria-hidden="true" />
              </div>
              <div>
                <h2 id={createTitleId} className={styles.dialogTitle}>
                  Create Named Config
                </h2>
                <p className={styles.dialogSubtitle}>
                  Create a reusable configuration profile template for{" "}
                  {agent === "codex" ? "Codex" : "Claude"}.
                </p>
              </div>
            </div>
            <div className={styles.dialogField}>
              <label htmlFor="create-config-name" className={styles.fieldLabel}>
                Config Name
              </label>
              <TextInput
                id="create-config-name"
                autoFocus
                aria-label="Named Config name"
                placeholder="e.g. team-staging, dev-fast"
                value={newName}
                onChange={(event) => changeNewName(event.target.value)}
                aria-invalid={newName.length > 0 && (!createNameValid || createNameTaken)}
                aria-describedby={createHelpId}
              />
              <div className={styles.pathPreview}>
                <span className={styles.pathPreviewLabel}>Storage Path:</span>
                <code>{`~/.aibox/${agent}/${tenant.kind === "host" ? "host" : tenant.name}/${newName.trim() || "<name>"}`}</code>
              </div>
            </div>
            <p id={createHelpId} className={layout.dialogDescription}>
              Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or
              number.
            </p>
            {newName.length > 0 && !createNameValid && (
              <AlertBanner
                className={styles.dialogAlert}
                tone="danger"
                icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
              >
                Enter a valid lowercase DNS label.
              </AlertBanner>
            )}
            {createNameValid && createNameTaken && (
              <AlertBanner
                className={styles.dialogAlert}
                tone="danger"
                icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
              >
                Named Config {newName} already exists.
              </AlertBanner>
            )}
            {createError && (
              <AlertBanner
                className={styles.dialogAlert}
                tone="danger"
                icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
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
      )}
      {applyTarget && (
        <ConfirmDialog
          title={`Apply ${applyTarget.name} to Current Config?`}
          description={
            <ApplyConfigDescription
              tenantLabel={configTenantLabel}
              isHost={tenant.kind === "host"}
              agentLabel={agent === "codex" ? "Codex" : "Claude"}
              sourceName={applyTarget.name}
            />
          }
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
          pills={
            <>
              <ContextPill
                icon={
                  tenant.kind === "host" ? (
                    <HostTenantIcon size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                  )
                }
                label="Tenant"
                value={configTenantLabel}
              />
              <ContextPill
                icon={<BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />}
                label="Agent"
                value={agent === "codex" ? "Codex" : "Claude"}
              />
            </>
          }
          message="Permanently deletes this profile. Current Config is unchanged."
          confirmLabel="Delete"
          busy={mutationBusy}
          onCancel={cancelDelete}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title={`Delete ${deleteTarget.names.length} selected Named Configs?`}
          pills={
            <>
              <ContextPill
                icon={
                  tenant.kind === "host" ? (
                    <HostTenantIcon size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                  )
                }
                label="Tenant"
                value={configTenantLabel}
              />
              <ContextPill
                icon={<BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />}
                label="Agent"
                value={agent === "codex" ? "Codex" : "Claude"}
              />
            </>
          }
          message="Permanently deletes the selected profiles. Current Config is unchanged."
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
              {preview ? "Propagate credentials?" : "Credential Propagation result"}
            </h2>
            <p className={styles.propagationSource}>
              Copies the ChatGPT credentials in Host Codex Current Config <code>auth.json</code> to
              older same-account Codex targets. Nothing else is read or written.
            </p>
            {(report || preview) && (
              <AlertBanner
                variant="inline"
                className={styles.propagationSummary}
                tone={
                  report
                    ? propagationHasFailures
                      ? "danger"
                      : propagationNeedsAttention
                        ? "warning"
                        : "success"
                    : preview?.preview.updates
                      ? "info"
                      : "neutral"
                }
                role={propagationHasFailures ? "alert" : "status"}
              >
                {report
                  ? propagationHasFailures
                    ? "Partially completed. Successful credential updates were kept; failed targets need attention."
                    : propagationNeedsAttention
                      ? "Credential propagation completed with targets that need attention."
                      : "Credential propagation completed."
                  : preview?.preview.updates
                    ? `${preview.preview.updates} target${preview.preview.updates === 1 ? "" : "s"} will receive the source credentials.`
                    : "No target needs these credentials. Nothing will be written."}
              </AlertBanner>
            )}
            <div className={styles.propagationGroups}>
              {propagationGroups.map((group) => {
                const entries = (preview?.preview.entries ?? report?.entries ?? []).filter(
                  (entry) => propagationGroup(entry.outcome.status) === group,
                );
                if (entries.length === 0) return null;
                return (
                  <section key={group}>
                    <h3>
                      {propagationGroupHeading(group, preview !== null)}{" "}
                      <span>{entries.length}</span>
                    </h3>
                    <div className={styles.propagationList}>
                      {entries.map((entry) => {
                        const status = propagationStatus(entry.outcome.status, preview !== null);
                        const detail = propagationDetail(entry.outcome, preview !== null);
                        return (
                          <div key={entry.label}>
                            <code>{entry.label}</code>
                            <StatusBadge tone={status.tone} variant="inline">
                              {status.label}
                            </StatusBadge>
                            <small>{detail}</small>
                          </div>
                        );
                      })}
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
                  tone="primary"
                  disabled={mutationBusy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  {busy && <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />}
                  {busy
                    ? "Propagating…"
                    : `Propagate to ${preview.preview.updates} target${preview.preview.updates === 1 ? "" : "s"}`}
                </ActionButton>
              )}
            </div>
          </section>
        </Dialog>
      )}
    </>
  );
}
