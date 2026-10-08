import { useTenantMutations } from "@/features/tenants/mutation/useTenantMutations";
import type { TenantPageProps, TenantViewModel } from "@/features/tenants/viewTypes";

import { useCallback, useEffect, useId, useMemo, useReducer, useRef, useState } from "react";

import type { ComponentKind } from "@/api/tenants";
import { allSelected } from "@/features/common/catalogSelection";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useSelectionModeFocus } from "@/features/common/useSelectionModeFocus";
import { hostTenant, managedTenants } from "@/features/common/tenantOptions";
import { parseComponentKind } from "@/features/tenants/components/componentCatalog";
import { fallbackTenantSelectionValue } from "@/features/tenants/route";
import { useComponents } from "@/features/tenants/components/useComponents";
import { useTenantCatalog } from "@/features/tenants/catalog/useTenantCatalog";
import { initialTenantWorkflow, tenantWorkflowReducer } from "@/features/tenants/tenantWorkflow";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";

import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";

import { abbreviateTenantHome } from "@/shared/lib/hostHome";

import {
  parseTenantSelectionValue,
  tenantSelectionValue,
  type TenantSelectionValue,
} from "@/domain/tenant";

export function useTenantController({
  api,
  operation,
  search,
  onLocationChange,
  onOperation,
}: TenantPageProps): TenantViewModel {
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
  const { dismissNotification, notifications, reportFailure } =
    useFailureNotifications("Tenants API call failed");
  const reportActionFailure = useCallback(
    (title: string, cause: unknown) => reportFailure("action", title, cause),
    [reportFailure],
  );
  const [refreshing, setRefreshing] = useState(false);
  const [componentAttention, setComponentAttention] = useState<{
    tenant: TenantSelectionValue;
    kind: ComponentKind;
  } | null>(() => {
    const query = new URLSearchParams(search);
    const kind = parseComponentKind(query.get("component"));
    const tenant = parseTenantSelectionValue(query.get("tenant"));
    return kind && tenant ? { tenant, kind } : null;
  });
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const tenantRows = useElementRegistry<HTMLButtonElement, TenantSelectionValue>();
  const createTitleId = useId();
  const createHelpId = useId();
  const [copiedHome, copyHome] = useClipboardFeedback<string>();
  const selectedKey = routedKey ?? fallbackTenantSelectionValue(tenants);
  const detailOpen = routedKey !== null;
  const selected = tenants.find((row) => tenantSelectionValue(row) === selectedKey) ?? null;
  const componentActions = useComponents({
    api,
    loadTenants,
    operation,
    onOperation,
    selected,
    setReadError: setError,
    reportActionFailure,
  });
  const selectedHostTenant = hostTenant(tenants);
  const sortedManagedTenants = useMemo(() => managedTenants(tenants), [tenants]);
  const selectableKeys = sortedManagedTenants
    .filter((row) => row.name !== "default")
    .map((row) => tenantSelectionValue(row));
  const allSelectable = allSelected(selectableKeys, selectedKeys);
  const { enterSelection, cancelSelection } = useSelectionModeFocus({
    selectionMode,
    selectButton,
    fallbackButton: refreshButton,
    focusFirstSelectable: () => selectableKeys.some((key) => tenantRows.focus(key)),
    onEnter: () => dispatchWorkflow({ type: "selection_enter" }),
    onExit: () => dispatchWorkflow({ type: "selection_cancel" }),
  });
  const selectedCount = selectedKeys.size;
  const { createTenant, deleteTenants, requestTenantDelete, createNameValid, createNameTaken } =
    useTenantMutations({
      api,
      workflow,
      dispatchWorkflow,
      tenants,
      sortedManagedTenants,
      loadTenants,
      onLocationChange,
      focusTenantRow: tenantRows.focus,
      reportActionFailure,
    });
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
    if (!query.has("component")) {
      normalizedComponentSearch.current = null;
      return;
    }
    if (normalizedComponentSearch.current === search) return;
    const kind = parseComponentKind(query.get("component"));
    const tenant = parseTenantSelectionValue(query.get("tenant"));
    if (kind && tenant) {
      // Keep the kind after the query is dropped so the row can highlight once.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setComponentAttention({ tenant, kind });
      if (!selected || componentActions.components.componentCatalogLoading) return;
    }
    query.delete("component");
    normalizedComponentSearch.current = search;
    onLocationChange(query, true);
  }, [componentActions.components.componentCatalogLoading, onLocationChange, search, selected]);
  useEffect(() => {
    if (componentAttention && selectedKey !== componentAttention.tenant) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setComponentAttention(null);
    }
  }, [componentAttention, selectedKey]);
  const attentionRowReady =
    componentAttention !== null &&
    selected !== null &&
    !componentActions.components.componentCatalogLoading &&
    selectedKey === componentAttention.tenant &&
    componentActions.components.componentGroups.some((group) =>
      group.rows.some((row) => row.kind === componentAttention.kind),
    );
  useEffect(() => {
    if (!componentAttention) return;
    if (!selected || selectedKey !== componentAttention.tenant) return;
    if (componentActions.components.componentCatalogLoading) return;
    if (!attentionRowReady) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setComponentAttention(null);
      return;
    }
    const timer = window.setTimeout(() => setComponentAttention(null), 2400);
    return () => window.clearTimeout(timer);
  }, [
    attentionRowReady,
    componentAttention,
    componentActions.components.componentCatalogLoading,
    selected,
    selectedKey,
  ]);
  useNarrowDetailFocus(detailHeadingRef, detailOpen && selectedKey !== null, selectedKey);
  useEffect(() => {
    if (loadingTenants) return;
    if (routedKey && !tenants.some((row) => tenantSelectionValue(row) === routedKey)) {
      onLocationChange(new URLSearchParams(), true);
    }
    // Catalog refreshes prune batch selections that no longer exist.
    dispatchWorkflow({
      type: "selection_prune",
      available: new Set(
        tenants
          .map((row) => tenantSelectionValue(row))
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

  function toggleTenant(key: TenantSelectionValue) {
    if (key === "host" || key === "managed:default") return;
    dispatchWorkflow({ type: "selection_toggle", key });
  }

  function toggleAllTenants() {
    dispatchWorkflow({ type: "selection_toggle_all", keys: selectableKeys, clear: allSelectable });
  }

  return {
    catalog: {
      hostTenant: selectedHostTenant,
      loadingTenants,
      managedTenants: sortedManagedTenants,
      refreshButton,
      refreshing,
      refreshTenants,
      retryTenantPage,
      selectButton,
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
      enterSelection,
      focusTenantRow: tenantRows.focus,
      registerTenantRow: tenantRows.register,
      toggleAllTenants,
      toggleTenant,
    },
    components: {
      ...componentActions.components,
      attentionKind:
        componentAttention && selectedKey === componentAttention.tenant
          ? componentAttention.kind
          : null,
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
      createNameTaken,
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
      dismissNotification,
      error,
      notifications,
    },
  };
}
