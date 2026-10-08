import { useCallback, useEffect, useId, useMemo, useReducer, useRef, useState } from "react";

import type { ConfigListData } from "@/api/configs";
import type { TenantRow } from "@/api/core";

import type { AgentKind } from "@/domain/agent";
import {
  DNS_LABEL_PATTERN,
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelectionValue,
} from "@/domain/tenant";
import {
  configLocation,
  namedConfigName,
  readConfigRoute,
  type ConfigSelection,
} from "@/features/common/routes/configs";
import { allSelected } from "@/features/common/catalogSelection";
import {
  agentSelectionOptions,
  tenantSelectionLabel,
  tenantSelectionOptions,
} from "@/features/common/tenantOptions";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useSelectionModeFocus } from "@/features/common/useSelectionModeFocus";
import { useConfigCatalog } from "@/features/configs/catalog/useConfigCatalog";
import { configWorkflowReducer, initialConfigWorkflow } from "@/features/configs/configWorkflow";
import { useConfigDetailLifecycle } from "@/features/configs/editor/useConfigDetailLifecycle";
import { useConfigEditorSession } from "@/features/configs/editor/useConfigEditorSession";
import { useConfigCrud } from "@/features/configs/mutation/useConfigCrud";
import { useCredentialPropagation } from "@/features/configs/mutation/useCredentialPropagation";
import type { ConfigPageProps, ConfigViewModel } from "@/features/configs/viewTypes";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { useFailureNotifications } from "@/shared/hooks/useFailureNotifications";

export function useConfigController({
  api,
  operation,
  search,
  onDirtyChange,
  onCancelLeave,
  onContinueLeave,
  onLocationChange,
  pendingLeave,
}: ConfigPageProps): ConfigViewModel {
  const route = useMemo(() => readConfigRoute(search), [search]);
  const { agent, detailOpen, selection, tenant } = route;
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [error, setError] = useState<string | null>(null);
  const { dismissNotification, notifications, reportFailure } =
    useFailureNotifications("Configs API call failed");
  const reportActionFailure = useCallback(
    (title: string, cause: unknown) => reportFailure("action", title, cause),
    [reportFailure],
  );
  const [workflow, dispatchWorkflow] = useReducer(configWorkflowReducer, initialConfigWorkflow);
  const { mutationBusy: busy, selectedKeys, selectionMode } = workflow;
  const onBusyChange = useCallback(
    (nextBusy: boolean) => dispatchWorkflow({ type: "mutation_changed", busy: nextBusy }),
    [],
  );
  const resetSelection = useCallback(() => dispatchWorkflow({ type: "selection_cancel" }), []);
  const recoverSelection = useCallback(
    (remaining: Set<string>, resume: boolean) =>
      dispatchWorkflow({ type: "selection_recovered", remaining, resume }),
    [],
  );
  const onCatalogLoaded = useCallback((data: ConfigListData) => {
    dispatchWorkflow({
      type: "selection_prune",
      available: new Set(data.configs.map((entry) => entry.name)),
    });
    setError(null);
  }, []);
  const {
    catalog,
    loading: loadingCatalog,
    refreshing,
    error: catalogError,
    load: loadCatalog,
  } = useConfigCatalog(api, tenant, agent, onCatalogLoaded);
  useEffect(() => {
    if (!catalog) return;
    const inspectedName = namedConfigName(selection);
    if (inspectedName && !catalog.configs.some((entry) => entry.name === inspectedName)) {
      onLocationChange(configLocation(tenant, agent, null), true);
    } else if (route.file && !catalog.files.includes(route.file)) {
      onLocationChange(
        configLocation(tenant, agent, detailOpen ? selection : null, catalog.files[0]),
        true,
      );
    }
  }, [agent, catalog, detailOpen, onLocationChange, route.file, selection, tenant]);
  const configRows = useElementRegistry<HTMLButtonElement>();
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const unsavedTitleId = useId();
  const createTitleId = useId();
  const createHelpId = useId();
  const propagationTitleId = useId();
  const operationRunning = operation?.state === "running";
  const mutationBusy = busy || operationRunning;
  const managedTenantMissing =
    !loadingTenants &&
    tenant.kind === "managed" &&
    !tenants.some((row) => row.kind === "managed" && row.name === tenant.name && row.exists);
  useEffect(() => {
    if (!managedTenantMissing || !detailOpen) return;
    // The latest Tenant catalog invalidated the route-backed detail selection.
    onLocationChange(configLocation(tenant, agent, null), true);
  }, [agent, detailOpen, managedTenantMissing, onLocationChange, tenant]);
  const tenantOptions = useMemo(() => tenantSelectionOptions(tenants), [tenants]);
  const agentOptions = useMemo(() => agentSelectionOptions(), []);
  const configTenantLabel = tenantSelectionLabel(tenants, tenant);
  const inspectedName = namedConfigName(selection);
  const configSelectionLabel = selection.current
    ? "Current Config"
    : inspectedName
      ? `Named Config ${inspectedName}`
      : "Named Configs";
  const currentSelection = selection.current;
  const selectedTenantSelectionValue = tenantSelectionValue(tenant);
  const selectedConfigKey = selection.current
    ? "current"
    : inspectedName
      ? `named:${inspectedName}`
      : "named-catalog";
  const configFiles = catalog?.files ?? [];
  const file =
    route.file && configFiles.includes(route.file) ? route.file : (configFiles[0] ?? null);
  const saveOrder = agent === "codex" ? ["auth.json", "config.toml"] : configFiles;
  const handlePaneSaved = useCallback(() => {
    void loadCatalog("background");
  }, [loadCatalog]);
  const {
    session,
    state: editorState,
    cancelPending,
  } = useConfigEditorSession({
    api,
    target: { tenant, agent, current: selection.current, config: namedConfigName(selection) },
    files: configFiles,
    enabled:
      Boolean(catalog) && !loadingTenants && !managedTenantMissing && !selection.namedCatalog,
    onDirtyChange,
    onError: setError,
    onSaved: handlePaneSaved,
    leave: { pending: pendingLeave, onCancel: onCancelLeave, onContinue: onContinueLeave },
  });
  const { pendingAction, mode: editorMode, visualAvailable } = editorState;
  const dirtyFiles = session.dirtyFiles;
  const fileStatuses = Object.fromEntries(
    configFiles.map((name) => {
      const { dirty, canSave } = session.file(name);
      return [name, { dirty, canSave }];
    }),
  );
  const {
    discardPending: discardAndRunPendingAction,
    reloadFiles,
    requestAction: requestEditorAction,
    retryReveals,
    saveInOrder,
    savePending,
    switchEditorMode,
    showRawEditor,
  } = session;
  const crud = useConfigCrud({
    agent,
    api,
    currentSelection,
    file,
    loadCatalog,
    onLocationChange,
    operationRunning,
    onBusyChange,
    onSelectionRecovery: recoverSelection,
    onSelectionReset: resetSelection,
    reloadFiles,
    requestEditorAction,
    selection,
    selectionMode,
    reportActionFailure,
    tenant,
  });
  const propagation = useCredentialPropagation({
    api,
    loadCatalog,
    onBusyChange,
    operationRunning,
    reportActionFailure,
  });
  const { closeConfigDetail, detailBackButtonRef, detailHeadingRef, registerPane } =
    useConfigDetailLifecycle({
      agent,
      selection,
      tenant,
      detailOpen,
      selectedConfigKey,
      selectedTenantSelectionValue,
      file,
      catalog,
      focusConfigRow: configRows.focus,
      requestEditorAction,
      resetSelection,
      onLocationChange,
    });
  const appliedName = catalog?.application.last_application?.applied ?? null;
  const selectedCount = selectedKeys.size;
  const selectableNames = catalog?.configs.map((entry) => entry.name) ?? [];
  const allSelectable = allSelected(selectableNames, selectedKeys);
  const { enterSelection, cancelSelection } = useSelectionModeFocus({
    selectionMode,
    selectButton,
    fallbackButton: refreshButton,
    focusFirstSelectable: () => selectableNames.some((name) => configRows.focus(name)),
    onEnter: () => dispatchWorkflow({ type: "selection_enter" }),
    onExit: resetSelection,
  });
  async function refreshConfigs() {
    if (await loadCatalog("refresh")) reloadFiles(configFiles);
  }
  function selectTenant(values: ReadonlySet<TenantSelectionValue>) {
    const next = [...values][0];
    if (!next || next === tenantSelectionValue(tenant)) return;
    requestEditorAction(() => {
      resetSelection();
      onLocationChange(configLocation(tenantSelectionFromValue(next), agent, null));
    });
  }
  function selectAgent(values: ReadonlySet<AgentKind>) {
    const next = [...values][0];
    if (!next || next === agent) return;
    requestEditorAction(() => {
      resetSelection();
      onLocationChange(configLocation(tenant, next, null));
    });
  }
  function openConfig(name: string) {
    requestEditorAction(() => {
      const nextSelection: ConfigSelection = { current: false, config: name };
      onLocationChange(configLocation(tenant, agent, nextSelection, file));
    });
  }
  function openCurrent() {
    requestEditorAction(() => {
      onLocationChange(configLocation(tenant, agent, { current: true }, file));
    });
  }
  function toggleConfig(name: string) {
    dispatchWorkflow({ type: "selection_toggle", key: name });
  }
  function toggleAllConfigs() {
    dispatchWorkflow({
      type: "selection_toggle_all",
      keys: selectableNames,
      clear: allSelectable,
    });
  }
  async function saveAll() {
    onBusyChange(true);
    try {
      if (!(await saveInOrder(saveOrder))) return;
      await loadCatalog("background");
    } finally {
      onBusyChange(false);
    }
  }
  const createNameValid = DNS_LABEL_PATTERN.test(crud.dialogs.newName);
  const createNameTaken = (catalog?.configs ?? []).some((row) => row.name === crud.dialogs.newName);
  return {
    catalog: {
      agent,
      agentOptions,
      catalog,
      catalogError,
      configFiles,
      configSelectionLabel,
      configTenantLabel,
      fileStatuses,
      loadCatalog,
      loadingCatalog,
      loadingTenants,
      managedTenantMissing,
      refreshing,
      refreshConfigs,
      retryTenants,
      selectAgent,
      selectTenant,
      tenant,
      tenantError,
      tenantOptions,
    },
    detail: {
      closeConfigDetail,
      detailBackButtonRef,
      detailHeadingRef,
      detailOpen,
      openConfig,
      openCurrent,
      selection,
    },
    selection: {
      allSelectable,
      cancelSelection,
      refreshButton,
      registerConfigRow: configRows.register,
      selectButton,
      selectableNames,
      selectedCount,
      selectedKeys,
      selectionMode,
      enterSelection,
      toggleAllConfigs,
      toggleConfig,
    },
    mutations: {
      ...crud.mutations,
      ...propagation.mutations,
      busy,
      mutationBusy,
      saveAll,
      saveInOrder,
      saveOrder,
      savePending,
    },
    dialogs: {
      ...crud.dialogs,
      ...propagation.dialogs,
      cancelPending,
      createHelpId,
      createNameTaken,
      createNameValid,
      createTitleId,
      discardAndRunPendingAction,
      pendingAction,
      propagationTitleId,
      unsavedTitleId,
    },
    editor: {
      session,
      dirtyFiles,
      editorMode,
      registerPane,
      requestEditorAction,
      retryReveals,
      showRawEditor,
      switchEditorMode,
      visualAvailable,
    },
    feedback: {
      appliedName,
      applyFeedback: crud.applyFeedback,
      dismissNotification,
      error,
      notifications,
      setError,
    },
  };
}
