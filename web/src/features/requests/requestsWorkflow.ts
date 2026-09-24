import {
  catalogSelectionReducer,
  initialCatalogSelection,
  type CatalogSelectionAction,
  type CatalogSelectionState,
} from "@/features/common/catalogSelection";

/** Which Request, or how many, a confirmation dialog is about. */
export type RequestsDialog =
  { kind: "batch"; ids: string[] } | { kind: "request"; id: string } | null;

/** The delete currently in flight, if any. */
export type RequestsDeletion = { kind: "batch" } | { kind: "request"; id: string } | null;

/**
 * Requests batch selection, its confirmation dialog, and the delete in flight.
 *
 * The selection context is the page number a Request was selected on. Requests
 * paginates, so a batch can span pages the list no longer shows; after a delete
 * the page returns to the earliest page it touched.
 */
export interface RequestsWorkflowState extends CatalogSelectionState<string, number> {
  deletion: RequestsDeletion;
  dialog: RequestsDialog;
}

export const initialRequestsWorkflow: RequestsWorkflowState = {
  ...initialCatalogSelection<string, number>(),
  deletion: null,
  dialog: null,
};

export type RequestsWorkflowAction =
  | CatalogSelectionAction<string, number>
  | { type: "dialog_opened"; dialog: Exclude<RequestsDialog, null> }
  | { type: "dialog_dismissed" }
  | { type: "delete_started"; deletion: Exclude<RequestsDeletion, null> }
  | { type: "delete_finished" };

export function requestsWorkflowReducer(
  state: RequestsWorkflowState,
  action: RequestsWorkflowAction,
): RequestsWorkflowState {
  switch (action.type) {
    case "dialog_opened":
      return { ...state, dialog: action.dialog };
    case "dialog_dismissed":
      return { ...state, dialog: null };
    case "delete_started":
      return { ...state, deletion: action.deletion };
    case "delete_finished":
      return { ...state, deletion: null };
    default:
      return { ...state, ...catalogSelectionReducer(state, action) };
  }
}

/** The earliest page any selected Request in `ids` was selected on. */
export function earliestSelectedPage(
  state: RequestsWorkflowState,
  ids: readonly string[],
  fallback: number,
): number {
  return ids.reduce(
    (minimum, id) => Math.min(minimum, state.selectionContexts.get(id) ?? fallback),
    Number.MAX_SAFE_INTEGER,
  );
}
