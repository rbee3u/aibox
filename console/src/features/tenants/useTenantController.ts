import type { RefObject } from "react";
import { useEffect, useId, useMemo, useReducer, useRef, useState } from "react";

import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type {
  ComponentKind,
  ComponentLatestSnapshot,
  ComponentRow,
  TenantApi,
} from "@/api/tenants";
import { allSelected } from "@/features/common/catalogSelection";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { hostTenant, managedTenants } from "@/features/common/tenantOptions";
import type { ComponentGroup } from "@/features/tenants/componentCatalog";
import {
  fallbackTenantSelectionValue,
  tenantSelectionValueOf,
  tenantLocation,
} from "@/features/tenants/route";
import {
  useComponentActions,
  type ComponentActionProgress,
  type ComponentRemoveTarget,
  type ComponentSpecificVersionTarget,
} from "@/features/tenants/mutation/useComponentActions";
import { useTenantCatalog } from "@/features/tenants/catalog/useTenantCatalog";
import {
  initialTenantWorkflow,
  tenantWorkflowReducer,
  type TenantDeleteTarget,
} from "@/features/tenants/tenantWorkflow";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import { messageOf } from "@/shared/lib/errors";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import {
  DNS_LABEL_PATTERN,
  parseTenantSelectionValue,
  type TenantSelectionValue,
} from "@/domain/tenant";

interface ControllerOptions {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export interface TenantViewModel {
  catalog: {
    hostTenant: TenantRow | null;
    loadingTenants: boolean;
    managedTenants: Array<TenantRow & { kind: "managed"; name: string }>;
    refreshing: boolean;
    refreshTenants: () => Promise<void>;
    retryTenantPage: () => Promise<void>;
    tenantCatalogError: string | null;
  };
  detail: {
    copiedHome: string | null;
    copyHome: (text: string, value: string) => Promise<void>;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailOpen: boolean;
    selected: TenantRow | null;
    selectedHome: string;
    selectedKey: TenantSelectionValue | null;
    tenantKindLabel: string;
  };
  selection: {
    allSelectable: boolean;
    cancelSelection: () => void;
    selectedCount: number;
    selectedKeys: Set<TenantSelectionValue>;
    selectableKeys: TenantSelectionValue[];
    selectionMode: boolean;
    enterSelection: () => void;
    focusTenantRow: (key: TenantSelectionValue) => void;
    registerTenantRow: (key: TenantSelectionValue, element: HTMLButtonElement | null) => void;
    toggleAllTenants: () => void;
    toggleTenant: (key: TenantSelectionValue) => void;
  };
  components: {
    attentionComponentCount: number;
    checkingLatest: boolean;
    checkForUpdates: () => Promise<void>;
    closeComponentMenu: () => void;
    componentActionProgress: ComponentActionProgress | null;
    componentCatalogLoading: boolean;
    componentGroups: Array<ComponentGroup & { rows: ComponentRow[] }>;
    componentMenuPosition: { top: number; left: number } | null;
    componentMenuRef: RefObject<HTMLDivElement | null>;
    componentTotalCount: number;
    installedComponentCount: number;
    isComponentExpanded: (kind: ComponentKind) => boolean;
    latestSnapshot: ComponentLatestSnapshot | null;
    loadComponents: (target: TenantRow | null, showLoading?: boolean) => Promise<void>;
    mutateComponent: (
      row: ComponentRow,
      install: boolean,
      requestedVersion?: string | null,
    ) => Promise<boolean>;
    openComponentMenu: (kind: ComponentKind, anchor: HTMLElement, width: number) => void;
    openMenu: ComponentKind | null;
    openSpecificVersion: (row: ComponentRow, mode: ComponentSpecificVersionTarget["mode"]) => void;
    registerComponentMenuButton: (kind: ComponentKind, element: HTMLButtonElement | null) => void;
    registerComponentMenuItem: (kind: ComponentKind, element: HTMLButtonElement | null) => void;
    submitSpecificVersion: () => Promise<void>;
    toggleComponentExpanded: (kind: ComponentKind) => void;
    toggleComponentMenu: (kind: ComponentKind, anchor: HTMLElement, width: number) => void;
  };
  mutations: {
    busy: boolean;
    createTenant: () => Promise<void>;
    deleteTenants: () => Promise<void>;
    mutationBusy: boolean;
    requestTenantDelete: (names: string[]) => void;
  };
  dialogs: {
    cancelComponentRemove: () => void;
    cancelDeleteDialog: () => void;
    changeNewName: (name: string) => void;
    changeSpecificVersion: (value: string) => void;
    closeCreateDialog: () => void;
    closeSpecificVersion: () => void;
    componentRemoveTarget: ComponentRemoveTarget | null;
    createError: string | null;
    createHelpId: string;
    createNameValid: boolean;
    createOpen: boolean;
    createTitleId: string;
    deleteTarget: TenantDeleteTarget | null;
    newName: string;
    openCreateDialog: () => void;
    removeComponent: () => Promise<void>;
    requestComponentRemove: (row: ComponentRow, tenantLabel: string) => void;
    specificVersion: string;
    specificVersionError: string | null;
    specificVersionHelpId: string;
    specificVersionTarget: ComponentSpecificVersionTarget | null;
    specificVersionTitleId: string;
    specificVersionValid: boolean;
    specificVersionValidationError: string | null;
  };
  feedback: {
    error: string | null;
  };
}

export function useTenantController({
  api,
  operation,
  search,
  onLocationChange,
  onOperation,
}: ControllerOptions): TenantViewModel {
  const normalizedComponentSearch = useRef<string | null>(null);
  const route = useMemo(() => new URLSearchParams(search), [search]);
  const routedKey = parseTenantSelectionValue(route.get("tenant"));
  const {
    tenants,
    loading: loadingTenants,
    error: tenantCatalogError,
    load: loadTenants,
  } = useTenantCatalog(api);
  const [workflow, dispatchWorkflow] = useReducer(tenantWorkflowReducer, initialTenantWorkflow);
  const {
    createError,
    createOpen,
    deleteTarget,
    mutationPhase,
    newName,
    selectedKeys,
    selectionMode,
  } = workflow;
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const tenantRows = useElementRegistry<HTMLButtonElement, TenantSelectionValue>();
  const createTitleId = useId();
  const createHelpId = useId();
  const [copiedHome, copyHome] = useClipboardFeedback<string>();
  const selectedKey = routedKey ?? fallbackTenantSelectionValue(tenants);
  const detailOpen = routedKey !== null;
  const selected = tenants.find((row) => tenantSelectionValueOf(row) === selectedKey) ?? null;
  const componentActions = useComponentActions({
    api,
    loadTenants,
    operation,
    onOperation,
    selected,
    setError,
  });
  const selectedHostTenant = hostTenant(tenants);
  const sortedManagedTenants = useMemo(() => managedTenants(tenants), [tenants]);
  const selectableKeys = sortedManagedTenants
    .filter((row) => row.name !== "default")
    .map((row) => tenantSelectionValueOf(row));
  const allSelectable = allSelected(selectableKeys, selectedKeys);
  const selectedCount = selectedKeys.size;
  const createNameValid = DNS_LABEL_PATTERN.test(newName);
  const busy = mutationPhase !== "idle";
  const combinedBusy = busy || componentActions.busy;
  const mutationBusy =
    combinedBusy ||
    operation?.state === "running" ||
    componentActions.componentActionProgress !== null;
  const selectedHome = selected
    ? abbreviateTenantHome(selected.home, selectedHostTenant?.home ?? null)
    : "";
  const tenantKindLabel = selected?.kind === "host" ? "Host Tenant" : "Managed Tenant";
  useEffect(() => {
    const query = new URLSearchParams(search);
    if (query.has("component") && normalizedComponentSearch.current !== search) {
      query.delete("component");
      normalizedComponentSearch.current = search;
      onLocationChange(query, true);
    } else if (!query.has("component")) {
      normalizedComponentSearch.current = null;
    }
  }, [onLocationChange, search]);
  useNarrowDetailFocus(detailHeadingRef, detailOpen && selectedKey !== null, selectedKey);
  useEffect(() => {
    if (loadingTenants) return;
    if (routedKey && !tenants.some((row) => tenantSelectionValueOf(row) === routedKey)) {
      onLocationChange(new URLSearchParams(), true);
    }
    // Catalog refreshes prune batch selections that no longer exist.
    dispatchWorkflow({
      type: "selection_prune",
      available: new Set(
        tenants
          .map((row) => tenantSelectionValueOf(row))
          .filter((key) => key !== "host" && key !== "managed:default"),
      ),
    });
  }, [loadingTenants, onLocationChange, routedKey, tenants]);
  async function refreshTenants() {
    setRefreshing(true);
    try {
      await loadTenants();
    } finally {
      setRefreshing(false);
    }
  }

  async function retryTenantPage() {
    setError(null);
    const rows = await loadTenants();
    if (rows) await componentActions.loadComponents(selected, true);
  }

  async function createTenant() {
    if (!createNameValid) return;
    dispatchWorkflow({ type: "create_started" });
    try {
      await api.createTenant(newName);
      const created = newName;
      dispatchWorkflow({ type: "create_succeeded" });
      await loadTenants();
      const key = `managed:${created}` as TenantSelectionValue;
      onLocationChange(tenantLocation(key));
    } catch (cause) {
      dispatchWorkflow({ type: "create_failed", message: messageOf(cause) });
    }
  }

  function toggleTenant(key: TenantSelectionValue) {
    if (key === "host" || key === "managed:default") return;
    dispatchWorkflow({ type: "selection_toggle", key });
  }

  function toggleAllTenants() {
    dispatchWorkflow({ type: "selection_toggle_all", keys: selectableKeys, clear: allSelectable });
  }

  function cancelSelection() {
    dispatchWorkflow({ type: "selection_cancel" });
  }

  function requestTenantDelete(names: string[]) {
    if (names.length === 0) return;
    dispatchWorkflow({ type: "delete_requested", names });
  }

  async function deleteTenants() {
    if (!deleteTarget || deleteTarget.names.length === 0) return;
    const requestedNames = deleteTarget.names;
    const wasSelectionMode = selectionMode;
    dispatchWorkflow({ type: "delete_started" });
    try {
      await api.deleteTenants(requestedNames);
      dispatchWorkflow({ type: "delete_succeeded" });
      await loadTenants();
    } catch (cause) {
      const deletionError = messageOf(cause);
      const refreshed = await loadTenants();
      if (refreshed) {
        const selectedStillExists =
          selectedKey !== null &&
          refreshed.some((row) => tenantSelectionValueOf(row) === selectedKey);
        if (selectedKey !== null && !selectedStillExists) componentActions.preserveNextError();
        const remaining = requestedNames.filter((name) =>
          refreshed.some((row) => row.kind === "managed" && row.name === name),
        );
        dispatchWorkflow({
          type: "delete_failed",
          remaining: remaining.map((name) => `managed:${name}` as TenantSelectionValue),
          resumeSelection: wasSelectionMode,
        });
      } else {
        dispatchWorkflow({ type: "delete_failed", remaining: [], resumeSelection: false });
      }
      setError(deletionError);
    }
  }

  return {
    catalog: {
      hostTenant: selectedHostTenant,
      loadingTenants,
      managedTenants: sortedManagedTenants,
      refreshing,
      refreshTenants,
      retryTenantPage,
      tenantCatalogError,
    },
    detail: {
      copiedHome,
      copyHome,
      detailHeadingRef,
      detailOpen,
      selected,
      selectedHome,
      selectedKey,
      tenantKindLabel,
    },
    selection: {
      allSelectable,
      cancelSelection,
      selectedCount,
      selectedKeys,
      selectableKeys,
      selectionMode,
      enterSelection: () => dispatchWorkflow({ type: "selection_enter" }),
      focusTenantRow: tenantRows.focus,
      registerTenantRow: tenantRows.register,
      toggleAllTenants,
      toggleTenant,
    },
    components: {
      ...componentActions.components,
      loadComponents: componentActions.loadComponents,
      componentActionProgress: componentActions.componentActionProgress,
    },
    mutations: {
      busy: combinedBusy,
      createTenant,
      deleteTenants,
      mutationBusy,
      requestTenantDelete,
    },
    dialogs: {
      ...componentActions.dialogs,
      createError,
      createHelpId,
      createNameValid,
      createOpen,
      createTitleId,
      deleteTarget,
      newName,
      cancelDeleteDialog: () => dispatchWorkflow({ type: "delete_cancelled" }),
      changeNewName: (name: string) => dispatchWorkflow({ type: "create_name_changed", name }),
      closeCreateDialog: () => dispatchWorkflow({ type: "create_close" }),
      openCreateDialog: () => dispatchWorkflow({ type: "create_open" }),
    },
    feedback: {
      error,
    },
  };
}
