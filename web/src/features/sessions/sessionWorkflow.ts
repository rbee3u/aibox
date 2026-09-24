import {
  catalogSelectionReducer,
  initialCatalogSelection,
  type CatalogSelectionAction,
  type CatalogSelectionState,
} from "@/features/common/catalogSelection";

/**
 * Session batch selection.
 *
 * Sessions carry no mutation phase of their own, so this is the shared
 * selection machine unchanged. It keeps a named module because the Session
 * pages, deletion hook, and their tests refer to it by name.
 */
export type SessionWorkflowState = CatalogSelectionState<string>;
export type SessionWorkflowAction = CatalogSelectionAction<string>;

export const initialSessionWorkflow: SessionWorkflowState = initialCatalogSelection<string>();

export const sessionWorkflowReducer = catalogSelectionReducer<string>;
