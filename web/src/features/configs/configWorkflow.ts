import {
  catalogSelectionReducer,
  initialCatalogSelection,
  type CatalogSelectionAction,
  type CatalogSelectionState,
} from "@/features/common/catalogSelection";

/** Config batch selection plus whether a Config mutation is in flight. */
export interface ConfigWorkflowState extends CatalogSelectionState<string> {
  mutationBusy: boolean;
}

export const initialConfigWorkflow: ConfigWorkflowState = {
  ...initialCatalogSelection<string>(),
  mutationBusy: false,
};

export type ConfigWorkflowAction =
  { type: "mutation_changed"; busy: boolean } | CatalogSelectionAction<string>;

export function configWorkflowReducer(
  state: ConfigWorkflowState,
  action: ConfigWorkflowAction,
): ConfigWorkflowState {
  if (action.type === "mutation_changed") {
    return { ...state, mutationBusy: action.busy };
  }
  return { ...state, ...catalogSelectionReducer(state, action) };
}
