import type { Operation } from "@/api/operations";
import { sessionTenantSelectionValue } from "@/features/sessions/route";
import { agentLabel, sessionListTenantLabel } from "@/features/sessions/sessionSource";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";
import { ConfirmDialog, ContextPill } from "@/shared/ui/ConfirmDialog";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

export function SessionDialogs({
  catalog,
  dialogs,
  feedback,
  mutations,
  operation,
}: Pick<SessionViewModel, "catalog" | "dialogs" | "feedback" | "mutations"> & {
  operation?: Operation | null;
}) {
  const { tenant, agent } = catalog;
  const { closeBatchDelete, closeSingleDelete, dialogKeys, singleDeleteTarget } = dialogs;
  const { batchBusy, deleteSelectedSessions, deleteSession, deletion } = mutations;

  const pills = (
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
        value={sessionListTenantLabel(sessionTenantSelectionValue(tenant))}
      />
      <ContextPill
        icon={<BrandIcon brand={brandForAgent(agent)} size={iconSize.xs} />}
        label="Agent"
        value={agentLabel(agent)}
      />
    </>
  );

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
          pills={pills}
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
          pills={pills}
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
