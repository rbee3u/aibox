import type { Operation } from "@/api/operations";
import { sessionDeletionFacts } from "@/features/sessions/sessionDeletion";
import { visibleSessionSource } from "@/features/sessions/sessionSource";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";

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
          facts={sessionDeletionFacts(singleDeleteTarget)}
          message={`Permanently deletes its Transcript from ${visibleSessionSource(singleDeleteTarget.source)}.`}
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
          message={`Permanently deletes the selected Transcripts. Sources: ${dialogSources
            .map(({ count, source }) => `${visibleSessionSource(source)} (${count})`)
            .join("; ")}.`}
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
