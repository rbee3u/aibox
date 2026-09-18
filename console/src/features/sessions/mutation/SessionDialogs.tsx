import type { Operation } from "@/api/operations";
import { sessionListTenantLabel } from "@/features/sessions/sessionSource";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";
import { ConfirmDialog, ContextPill } from "@/shared/ui/ConfirmDialog";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

export function SessionDialogs({
  dialogs,
  feedback,
  mutations,
  operation,
}: Pick<SessionViewModel, "dialogs" | "feedback" | "mutations"> & {
  operation?: Operation | null;
}) {
  const { closeBatchDelete, closeSingleDelete, dialogKeys, dialogSources, singleDeleteTarget } =
    dialogs;
  const { batchBusy, deleteSelectedSessions, deleteSession, deletion } = mutations;
  const singleSource = dialogSources.length === 1 ? dialogSources[0].source : null;

  return (
    <>
      <NotificationCenter
        notifications={feedback.notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogKeys !== null || singleDeleteTarget !== null}
        onAction={() => undefined}
        onDismiss={feedback.dismissNotification}
      />
      {singleDeleteTarget && (
        <ConfirmDialog
          title={`Delete Session ${singleDeleteTarget.display_id}?`}
          pills={
            <>
              <ContextPill
                icon={
                  singleDeleteTarget.source.tenant.kind === "host" ? (
                    <HostTenantIcon size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                  )
                }
                label="Tenant"
                value={sessionListTenantLabel(singleDeleteTarget.source.tenantSelectionValue)}
              />
              <ContextPill
                icon={
                  <BrandIcon
                    brand={brandForAgent(singleDeleteTarget.source.agent)}
                    size={iconSize.xs}
                  />
                }
                label="Agent"
                value={singleDeleteTarget.source.agentLabel}
              />
            </>
          }
          message="Permanently deletes this session transcript. This action cannot be undone."
          confirmLabel="Delete"
          busy={deletion?.kind === "record" || operation?.state === "running"}
          onCancel={() => {
            if (deletion?.kind !== "record") closeSingleDelete();
          }}
          onConfirm={() => void deleteSession(singleDeleteTarget)}
        />
      )}
      {dialogKeys && (
        <ConfirmDialog
          title={`Delete ${dialogKeys.length} selected Session${dialogKeys.length === 1 ? "" : "s"}?`}
          pills={
            singleSource ? (
              <>
                <ContextPill
                  icon={
                    singleSource.tenant.kind === "host" ? (
                      <HostTenantIcon size={iconSize.xs} aria-hidden="true" />
                    ) : (
                      <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                    )
                  }
                  label="Tenant"
                  value={sessionListTenantLabel(singleSource.tenantSelectionValue)}
                />
                <ContextPill
                  icon={<BrandIcon brand={brandForAgent(singleSource.agent)} size={iconSize.xs} />}
                  label="Agent"
                  value={singleSource.agentLabel}
                />
              </>
            ) : undefined
          }
          message="Permanently deletes the selected session transcripts. This action cannot be undone."
          confirmLabel="Delete"
          busy={batchBusy || operation?.state === "running"}
          onCancel={() => {
            if (!batchBusy) closeBatchDelete();
          }}
          onConfirm={() => void deleteSelectedSessions()}
        />
      )}
    </>
  );
}
