import type { Dispatch, RefObject, SetStateAction } from "react";
import { useCallback, useEffect, useId, useMemo, useReducer, useRef, useState } from "react";

import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import { CODING_AGENTS, type CodingAgentKind } from "@/domain/codingAgent";
import { DNS_LABEL_PATTERN, type TenantSelectionValue } from "@/domain/tenant";
import {
  agentSelectionOptions,
  tenantSelectionLabel,
  tenantSelectionOptions,
} from "@/features/common/tenantOptions";
import type { ConfigFileController } from "@/features/configs/detail/configFileController";
import {
  useConfigEditorSession,
  type ConfigFileStatus,
} from "@/features/configs/detail/useConfigEditorSession";
import { configWorkflowReducer, initialConfigWorkflow } from "@/features/configs/configWorkflow";
import {
  configLocation,
  configTenantSelectionValue,
  readConfigRoute,
  tenantSelectionFromConfigValue,
  type ConfigApplyTarget,
  type ConfigDeleteTarget,
  type ConfigPendingAction,
  type ConfigSelection,
} from "@/features/configs/route";
import { useConfigCatalog } from "@/features/configs/catalog/useConfigCatalog";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { useConfigCrud } from "@/features/configs/mutation/useConfigCrud";
import { useCredentialPropagation } from "@/features/configs/mutation/useCredentialPropagation";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";

interface ControllerOptions {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export interface ConfigViewModel {
  catalog: {
    agent: CodingAgentKind;
    agentOptions: SelectionOption<CodingAgentKind>[];
    catalog: ConfigListData | null;
    catalogError: string | null;
    configFiles: string[];
    configSelectionLabel: string;
    configTenantLabel: string;
    fileStatuses: Record<string, ConfigFileStatus>;
    loadCatalog: (kind?: ConfigCatalogLoadKind) => Promise<ConfigListData | null>;
    loadingCatalog: boolean;
    loadingTenants: boolean;
    managedTenantMissing: boolean;
    refreshing: boolean;
    retryTenants: () => void;
    selectAgent: (values: ReadonlySet<CodingAgentKind>) => void;
    selectTenant: (values: ReadonlySet<TenantSelectionValue>) => void;
    tenant: ReturnType<typeof tenantSelectionFromConfigValue>;
    tenantError: string | null;
    tenantOptions: SelectionOption<TenantSelectionValue>[];
  };
  detail: {
    closeConfigDetail: () => void;
    detailBackButtonRef: RefObject<HTMLButtonElement | null>;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailOpen: boolean;
    file: string | null;
    openConfig: (name: string) => void;
    openCurrent: () => void;
    selection: ConfigSelection;
  };
  selection: {
    allSelectable: boolean;
    cancelSelection: () => void;
    registerConfigRow: (key: string, element: HTMLButtonElement | null) => void;
    selectableNames: string[];
    selectedCount: number;
    selectedKeys: Set<string>;
    selectionMode: boolean;
    enterSelection: () => void;
    toggleAllConfigs: () => void;
    toggleConfig: (name: string) => void;
  };
  mutations: {
    applyConfig: (name: string) => Promise<void>;
    busy: boolean;
    createConfig: (name: string) => Promise<void>;
    deleteConfigs: () => Promise<void>;
    executePropagation: () => Promise<void>;
    mutationBusy: boolean;
    previewPropagation: () => Promise<void>;
    requestDelete: (names: string[]) => void;
    saveAll: () => Promise<void>;
    saveInOrder: (names: readonly string[]) => Promise<boolean>;
    saveOrder: string[];
    savePending: (names: readonly string[]) => Promise<void>;
  };
  dialogs: {
    applyTarget: ConfigApplyTarget | null;
    cancelApply: () => void;
    cancelDelete: () => void;
    cancelPending: () => void;
    changeNewName: (name: string) => void;
    closeCreateDialog: () => void;
    closePropagation: () => void;
    createError: string | null;
    createHelpId: string;
    createNameValid: boolean;
    createOpen: boolean;
    createTitleId: string;
    deleteTarget: ConfigDeleteTarget | null;
    discardAndRunPendingAction: () => Promise<void>;
    newName: string;
    openCreateDialog: () => void;
    pendingAction: ConfigPendingAction | null;
    preview: PropagationPreview | null;
    propagationHasFailures: boolean;
    propagationNeedsAttention: boolean;
    propagationTitleId: string;
    report: PropagationReport | null;
    requestApply: (name: string) => void;
    unsavedTitleId: string;
  };
  editor: {
    dirtyFiles: readonly string[];
    editorMode: "visual" | "raw";
    handleLinkedFileSaved: (name: string) => void;
    handlePaneSaved: () => void;
    handleVisualAvailable: (available: boolean) => void;
    registerPane: (name: string, element: HTMLDivElement | null) => void;
    prepareMainConfigSave: (customProvider: boolean) => boolean;
    registerFileController: (name: string, controller: ConfigFileController | null) => void;
    registerRevealRetry: (name: string, retry: (() => void) | null) => void;
    requestEditorAction: (action: () => void | Promise<void>) => void;
    retryReveals: () => void;
    showRawEditor: () => void;
    switchEditorMode: (next: "visual" | "raw") => void;
    visualAvailable: boolean;
  };
  feedback: {
    appliedName: string | null;
    applyFeedback: string | null;
    error: string | null;
    setError: Dispatch<SetStateAction<string | null>>;
  };
}

export function useConfigController({
  api,
  operation,
  search,
  onDirtyChange,
  onLocationChange,
}: ControllerOptions): ConfigViewModel {
  const route = useMemo(() => readConfigRoute(search), [search]);
  const { agent, detailOpen, selection, tenant } = route;
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [editorMode, setEditorMode] = useState<"visual" | "raw">("raw");
  const [visualAvailable, setVisualAvailable] = useState(false);
  const visualModeInitialized = useRef(false);
  const [error, setError] = useState<string | null>(null);
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
  const onCatalogLoaded = useCallback(
    (data: ConfigListData) => {
      if (!selection.current && !data.configs.some((entry) => entry.name === selection.config)) {
        onLocationChange(configLocation(tenant, agent, null), true);
      } else if (route.file && !data.files.includes(route.file)) {
        onLocationChange(
          configLocation(tenant, agent, detailOpen ? selection : null, data.files[0]),
          true,
        );
      }
      dispatchWorkflow({
        type: "selection_prune",
        available: new Set(data.configs.map((entry) => entry.name)),
      });
      setError(null);
    },
    [agent, detailOpen, onLocationChange, route.file, selection, tenant],
  );
  const {
    catalog,
    loading: loadingCatalog,
    refreshing,
    error: catalogError,
    load: loadCatalog,
  } = useConfigCatalog(api, tenant, agent, onCatalogLoaded);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const detailBackButtonRef = useRef<HTMLButtonElement>(null);
  const configRows = useElementRegistry<HTMLButtonElement>();
  const focusAfterDetailClose = useRef<string | null>(null);
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
  useNarrowDetailFocus(
    detailBackButtonRef,
    detailOpen,
    route.file,
    selection.current ? "current" : selection.config,
  );
  useEffect(() => {
    if (!managedTenantMissing || !detailOpen) return;
    // The latest Tenant catalog invalidated the route-backed detail selection.
    onLocationChange(configLocation(tenant, agent, null), true);
  }, [agent, detailOpen, managedTenantMissing, onLocationChange, tenant]);
  const tenantOptions = useMemo(() => tenantSelectionOptions(tenants), [tenants]);
  const agentOptions = useMemo(() => agentSelectionOptions(CODING_AGENTS), []);
  const configTenantLabel = tenantSelectionLabel(tenants, tenant);
  const configSelectionLabel = selection.current
    ? "Current Config"
    : `Named Config ${selection.config}`;
  const currentSelection = selection.current;
  const selectedTenantSelectionValue = configTenantSelectionValue(tenant);
  const selectedConfigKey = selection.current ? "current" : `named:${selection.config}`;
  const configFiles = catalog?.files ?? [];
  const file =
    route.file && configFiles.includes(route.file) ? route.file : (configFiles[0] ?? null);
  const saveOrder = agent === "codex" ? ["auth.json", "config.toml"] : configFiles;
  const {
    cancelPending,
    dirtyFiles,
    discardPending: discardAndRunPendingAction,
    fileStatuses,
    pendingAction,
    prepareMainConfigSave,
    registerController: registerFileController,
    registerRevealRetry,
    reloadFile: handleLinkedFileSaved,
    reloadFiles,
    requestAction: requestEditorAction,
    retryReveals,
    saveInOrder,
    savePending,
  } = useConfigEditorSession(
    configFiles,
    `${selectedTenantSelectionValue}:${agent}`,
    onDirtyChange,
    setError,
  );
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
    setError,
    tenant,
  });
  const propagation = useCredentialPropagation({
    api,
    loadCatalog,
    onBusyChange,
    operationRunning,
    setError,
  });
  const panes = useElementRegistry<HTMLDivElement>();
  function closeConfigDetail() {
    requestEditorAction(() => {
      focusAfterDetailClose.current = selection.current ? "current" : selection.config;
      onLocationChange(configLocation(tenant, agent, null));
    });
  }
  useEffect(() => {
    if (detailOpen || !focusAfterDetailClose.current) return;
    const key = focusAfterDetailClose.current;
    let focusFrame = 0;
    const revealFrame = window.requestAnimationFrame(() => {
      focusFrame = window.requestAnimationFrame(() => {
        if (configRows.focus(key)) focusAfterDetailClose.current = null;
      });
    });
    return () => {
      window.cancelAnimationFrame(revealFrame);
      window.cancelAnimationFrame(focusFrame);
    };
  }, [catalog, configRows, detailOpen]);
  useEffect(() => {
    visualModeInitialized.current = false;
    // A route-backed Config selection owns a distinct editor-mode lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setVisualAvailable(false);
    setEditorMode("raw");
  }, [agent, selectedConfigKey, selectedTenantSelectionValue]);
  useEffect(() => {
    if (!detailOpen || !file) return;
    const frame = window.requestAnimationFrame(() =>
      panes.get(file)?.scrollIntoView?.({ block: "nearest" }),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, file, catalog, panes]);
  useEffect(() => {
    // Loading a different external Config catalog resets editor-local state.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setVisualAvailable(false);
    setEditorMode("raw");
    resetSelection();
  }, [agent, resetSelection, selectedTenantSelectionValue]);
  const appliedName = catalog?.application.last_application?.applied ?? null;
  const selectedCount = selectedKeys.size;
  const selectableNames = catalog?.configs.map((entry) => entry.name) ?? [];
  const allSelectable =
    selectableNames.length > 0 && selectableNames.every((name) => selectedKeys.has(name));
  const handlePaneSaved = useCallback(() => {
    void loadCatalog("background");
  }, [loadCatalog]);
  const handleVisualAvailable = useCallback(
    (available: boolean) => {
      setVisualAvailable(available);
      if (available && !visualModeInitialized.current && !currentSelection) {
        visualModeInitialized.current = true;
        setEditorMode("visual");
      }
    },
    [currentSelection],
  );
  const switchEditorMode = useCallback(
    (next: "visual" | "raw") => {
      if (next === editorMode) return;
      if (next === "visual" && (!visualAvailable || currentSelection)) {
        setError("Visual Editor is available only for a valid Named Config main file.");
        return;
      }
      requestEditorAction(() => {
        setEditorMode(next);
        setError(null);
      });
    },
    [currentSelection, editorMode, requestEditorAction, visualAvailable],
  );
  const showRawEditor = useCallback(() => setEditorMode("raw"), []);
  function selectTenant(values: ReadonlySet<TenantSelectionValue>) {
    const next = [...values][0];
    if (!next || next === configTenantSelectionValue(tenant)) return;
    requestEditorAction(() => {
      resetSelection();
      onLocationChange(configLocation(tenantSelectionFromConfigValue(next), agent, null));
    });
  }
  function selectAgent(values: ReadonlySet<CodingAgentKind>) {
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
  function cancelSelection() {
    resetSelection();
  }
  async function saveAll() {
    onBusyChange(true);
    try {
      await saveInOrder(saveOrder);
      await loadCatalog("background");
    } finally {
      onBusyChange(false);
    }
  }
  const createNameValid = DNS_LABEL_PATTERN.test(crud.dialogs.newName);
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
      file,
      openConfig,
      openCurrent,
      selection,
    },
    selection: {
      allSelectable,
      cancelSelection,
      registerConfigRow: configRows.register,
      selectableNames,
      selectedCount,
      selectedKeys,
      selectionMode,
      enterSelection: () => dispatchWorkflow({ type: "selection_enter" }),
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
      createNameValid,
      createTitleId,
      discardAndRunPendingAction,
      pendingAction,
      propagationTitleId,
      unsavedTitleId,
    },
    editor: {
      dirtyFiles,
      editorMode,
      handleLinkedFileSaved,
      handlePaneSaved,
      handleVisualAvailable,
      prepareMainConfigSave,
      registerFileController,
      registerPane: panes.register,
      registerRevealRetry,
      requestEditorAction,
      retryReveals,
      showRawEditor,
      switchEditorMode,
      visualAvailable,
    },
    feedback: {
      appliedName,
      applyFeedback: crud.applyFeedback,
      error,
      setError,
    },
  };
}
