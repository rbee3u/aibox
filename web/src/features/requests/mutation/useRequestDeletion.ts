import type { Dispatch, RefObject, SetStateAction } from "react";
import type { RequestList, RequestsApi } from "@/api/requests";
import type {
  RequestsWorkflowState,
  RequestsWorkflowAction,
  RequestsDeletion,
} from "@/features/requests/requestsWorkflow";
import { earliestSelectedPage } from "@/features/requests/requestsWorkflow";
import type { RequestsRoute } from "@/features/requests/route";
import { focusTargetAfterDelete, removeDeletedFromList } from "@/features/requests/listModel";

interface RequestDeletionOptions {
  api: RequestsApi;
  workflow: RequestsWorkflowState;
  dispatchWorkflow: Dispatch<RequestsWorkflowAction>;
  deletionInProgress: RefObject<boolean>;
  cancelListRequest: () => void;
  list: RequestList;
  setList: Dispatch<SetStateAction<RequestList>>;
  currentId: string | null;
  setDetailOpen: Dispatch<SetStateAction<boolean>>;
  clearCurrentRequest: () => void;
  clearRequestIfCurrent: (id: string) => void;
  updateLocation: (route: RequestsRoute, replace?: boolean) => void;
  pageRef: RefObject<number>;
  refreshWithFallback: (page?: number) => Promise<{ page: number; payload: RequestList } | null>;
  setFocusAfterDelete: Dispatch<SetStateAction<string | null | undefined>>;
  resolveFailure: (source: "action") => void;
  reportFailure: (source: "action", title: string, cause: unknown) => void;
}

/** Request deletion, list reconciliation, dialog recovery, and focus planning. */
export function useRequestDeletion({
  api,
  workflow,
  dispatchWorkflow,
  deletionInProgress,
  cancelListRequest,
  list,
  setList,
  currentId,
  setDetailOpen,
  clearCurrentRequest,
  clearRequestIfCurrent,
  updateLocation,
  pageRef,
  refreshWithFallback,
  setFocusAfterDelete,
  resolveFailure,
  reportFailure,
}: RequestDeletionOptions) {
  const { dialog } = workflow;
  function beginDeletion(next: Exclude<RequestsDeletion, null>): boolean {
    if (deletionInProgress.current) return false;
    deletionInProgress.current = true;
    cancelListRequest();
    dispatchWorkflow({ type: "delete_started", deletion: next });
    return true;
  }

  function finishDeletion() {
    deletionInProgress.current = false;
    dispatchWorkflow({ type: "delete_finished" });
  }

  async function confirmDelete() {
    if (!dialog) return;
    if (dialog.kind === "request") {
      await deleteRequest(dialog.id);
      return;
    }
    if (!beginDeletion({ kind: "batch" })) return;
    resolveFailure("action");
    const targetPage = earliestSelectedPage(workflow, dialog.ids, pageRef.current);
    try {
      const deletedCount = await api.deleteRequests(dialog.ids);
      const deletedIds = dialog.ids;
      setList((current) =>
        removeDeletedFromList(current, deletedIds, deletedCount, pageRef.current),
      );
      // Leaves selection mode whether or not every id was removed. Sessions
      // resumes a partial selection instead; keeping the two different is
      // deliberate rather than an oversight.
      dispatchWorkflow({ type: "selection_cancel" });
      if (currentId && deletedIds.includes(currentId)) {
        setDetailOpen(false);
        clearCurrentRequest();
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      }
      dispatchWorkflow({ type: "dialog_dismissed" });
      resolveFailure("action");
      await refreshWithFallback(targetPage);
      setFocusAfterDelete(null);
    } catch (cause) {
      const title =
        dialog.ids.length === 1 ? "Couldn’t delete request" : "Couldn’t delete requests";
      dispatchWorkflow({ type: "dialog_dismissed" });
      reportFailure("action", title, cause);
    } finally {
      finishDeletion();
    }
  }

  async function deleteRequest(id: string) {
    if (!beginDeletion({ kind: "request", id })) return;
    const originPage = pageRef.current;
    const originRequests = list.requests;
    resolveFailure("action");
    try {
      await api.deleteRequests([id]);
      if (currentId === id) {
        setDetailOpen(false);
        clearCurrentRequest();
        updateLocation({ page: pageRef.current, request: null, tab: "summary" }, true);
      } else {
        clearRequestIfCurrent(id);
      }
      setList((current) => removeDeletedFromList(current, [id], 1, pageRef.current));
      setFocusAfterDelete(
        focusTargetAfterDelete(
          originRequests,
          id,
          originRequests.filter((request) => request.id !== id),
          false,
        ),
      );
      const refreshed = await refreshWithFallback(originPage);
      if (refreshed) {
        setFocusAfterDelete(
          focusTargetAfterDelete(
            originRequests,
            id,
            refreshed.payload.requests,
            refreshed.page !== originPage,
          ),
        );
      }
    } catch (cause) {
      reportFailure("action", "Couldn’t delete request", cause);
    } finally {
      dispatchWorkflow({ type: "dialog_dismissed" });
      finishDeletion();
    }
  }

  return { confirmDelete };
}
