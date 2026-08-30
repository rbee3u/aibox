import type { TenantSelectionValue } from "@/domain/tenant";
import {
  catalogSelectionReducer,
  initialCatalogSelection,
  type CatalogSelectionAction,
  type CatalogSelectionState,
} from "@/features/common/catalogSelection";

export interface TenantDeleteTarget {
  names: string[];
}

export type TenantMutationPhase = "idle" | "creating" | "deleting";

/** Tenant batch selection plus the create and delete dialog lifecycle. */
export interface TenantWorkflowState extends CatalogSelectionState<TenantSelectionValue> {
  createError: string | null;
  createOpen: boolean;
  deleteTarget: TenantDeleteTarget | null;
  mutationPhase: TenantMutationPhase;
  newName: string;
}

export const initialTenantWorkflow: TenantWorkflowState = {
  ...initialCatalogSelection<TenantSelectionValue>(),
  createError: null,
  createOpen: false,
  deleteTarget: null,
  mutationPhase: "idle",
  newName: "",
};

export type TenantWorkflowAction =
  | CatalogSelectionAction<TenantSelectionValue>
  | { type: "create_open" }
  | { type: "create_close" }
  | { type: "create_name_changed"; name: string }
  | { type: "create_started" }
  | { type: "create_succeeded" }
  | { type: "create_failed"; message: string }
  | { type: "delete_requested"; names: string[] }
  | { type: "delete_cancelled" }
  | { type: "delete_started" }
  | { type: "delete_succeeded" }
  | {
      type: "delete_failed";
      remaining: TenantSelectionValue[];
      resumeSelection: boolean;
    };

export function tenantWorkflowReducer(
  state: TenantWorkflowState,
  action: TenantWorkflowAction,
): TenantWorkflowState {
  switch (action.type) {
    case "create_open":
      return { ...state, createError: null, createOpen: true };
    case "create_close":
      return { ...state, createOpen: false };
    case "create_name_changed":
      return { ...state, createError: null, newName: action.name };
    case "create_started":
      return { ...state, mutationPhase: "creating" };
    case "create_succeeded":
      return {
        ...state,
        createError: null,
        createOpen: false,
        mutationPhase: "idle",
        newName: "",
      };
    case "create_failed":
      return { ...state, createError: action.message, mutationPhase: "idle" };
    case "delete_requested":
      return { ...state, deleteTarget: { names: action.names } };
    case "delete_cancelled":
      return { ...state, deleteTarget: null };
    case "delete_started":
      return { ...state, mutationPhase: "deleting" };
    case "delete_succeeded":
      return {
        ...state,
        ...catalogSelectionReducer(state, { type: "selection_cancel" }),
        deleteTarget: null,
        mutationPhase: "idle",
      };
    case "delete_failed":
      return {
        ...state,
        ...catalogSelectionReducer(state, {
          type: "selection_recovered",
          remaining: new Set(action.remaining),
          resume: action.resumeSelection,
        }),
        deleteTarget: null,
        mutationPhase: "idle",
      };
    default:
      return { ...state, ...catalogSelectionReducer(state, action) };
  }
}
