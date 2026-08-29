import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type { CodingAgentKind } from "@/domain/codingAgent";
import { DNS_LABEL_PATTERN, type TenantSelection, type TenantSelectionKey } from "@/domain/tenant";
import { propagationGroup } from "@/features/configs/configCatalog";
import { useConfigEditorSession } from "@/features/configs/editor/useConfigEditorSession";
import {
  configLocation,
  configTenantKey,
  readConfigRoute,
  tenantSelectionFromConfigKey,
  type ConfigApplyTarget,
  type ConfigDeleteTarget,
  type ConfigSelection,
} from "@/features/configs/route";
import { useConfigCatalog } from "@/features/configs/useConfigCatalog";
import { useAsyncResource } from "@/shared/hooks/useAsyncResource";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";

const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

interface ControllerOptions {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export function useConfigController({
  api,
  operation,
  search,
  onDirtyChange,
  onLocationChange,
}: ControllerOptions) {
  const [initialRoute] = useState(() => readConfigRoute(search));
  const observedSearch = useRef(search);
  const loadTenants = useCallback((signal: AbortSignal) => api.listTenants(signal), [api]);
  const {
    data: tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useAsyncResource<TenantRow[]>(loadTenants, []);
  const [tenant, setTenant] = useState<TenantSelection>(initialRoute.tenant);
  const [agent, setAgent] = useState<CodingAgentKind>(initialRoute.agent);
  const [selection, setSelection] = useState<ConfigSelection>(initialRoute.selection);
  const selectionRef = useRef<ConfigSelection>(initialRoute.selection);
  const [file, setFile] = useState<string | null>(initialRoute.file);
  const [editorMode, setEditorMode] = useState<"visual" | "raw">("raw");
  const [visualAvailable, setVisualAvailable] = useState(false);
  const visualModeInitialized = useRef(false);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedNames, setSelectedNames] = useState<Set<string>>(new Set());
  const [detailOpen, setDetailOpen] = useState(initialRoute.detailOpen);
  const onCatalogLoaded = useCallback(
    (data: ConfigListData) => {
      const routedSelection = selectionRef.current;
      if (
        !routedSelection.current &&
        !data.configs.some((entry) => entry.name === routedSelection.config)
      ) {
        const fallback: ConfigSelection = { current: true };
        selectionRef.current = fallback;
        setSelection(fallback);
        setDetailOpen(false);
        onLocationChange(configLocation(tenant, agent, null), true);
      }
      setFile((current) =>
        current && data.files.includes(current) ? current : (data.files[0] ?? null),
      );
      setSelectedNames(
        (current) =>
          new Set([...current].filter((name) => data.configs.some((entry) => entry.name === name))),
      );
      setError(null);
    },
    [agent, onLocationChange, tenant],
  );
  const {
    catalog,
    loading: loadingCatalog,
    refreshing,
    error: catalogError,
    load: loadCatalog,
  } = useConfigCatalog(api, tenant, agent, onCatalogLoaded);
  const [deleteTarget, setDeleteTarget] = useState<ConfigDeleteTarget | null>(null);
  const [applyTarget, setApplyTarget] = useState<ConfigApplyTarget | null>(null);
  const [applyFeedback, setApplyFeedback] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const detailBackButtonRef = useRef<HTMLButtonElement>(null);
  const configRowButtons = useRef(new Map<string, HTMLButtonElement>());
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);
  const unsavedTitleId = useId();
  const createTitleId = useId();
  const createHelpId = useId();
  const propagationTitleId = useId();
  const operationRunning = operation?.state === "running";
  const mutationBusy = busy || operationRunning;
  useEffect(() => {
    if (observedSearch.current === search) return;
    observedSearch.current = search;
    const route = readConfigRoute(search);
    setTenant((current) =>
      configTenantKey(current) === configTenantKey(route.tenant) ? current : route.tenant,
    );
    setAgent((current) => (current === route.agent ? current : route.agent));
    setSelection((current) => {
      const currentKey = current.current ? "current" : `named:${current.config}`;
      const routeKey = route.selection.current ? "current" : `named:${route.selection.config}`;
      return currentKey === routeKey ? current : route.selection;
    });
    setFile((current) => (current === route.file ? current : route.file));
    setDetailOpen((current) => (current === route.detailOpen ? current : route.detailOpen));
    setSelectionMode(false);
    setSelectedNames(new Set());
  }, [search]);
  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);
  const managedTenantMissing =
    !loadingTenants &&
    tenant.kind === "managed" &&
    !tenants.some((row) => row.kind === "managed" && row.name === tenant.name && row.exists);
  useEffect(() => {
    if (!detailOpen || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() =>
      (detailHeadingRef.current ?? detailBackButtonRef.current)?.focus(),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, file, selection]);
  useEffect(() => {
    if (!managedTenantMissing || !detailOpen) return;
    // The latest Tenant catalog invalidated the route-backed detail selection.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setDetailOpen(false);
    setFile(null);
    onLocationChange(configLocation(tenant, agent, null), true);
  }, [agent, detailOpen, managedTenantMissing, onLocationChange, tenant]);
  const tenantOptions = useMemo<SelectionOption<TenantSelectionKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter(
        (
          tenant,
        ): tenant is TenantRow & {
          kind: "managed";
          name: string;
        } => Boolean(tenant.kind === "managed" && tenant.name),
      )
      .sort((left, right) => left.name.localeCompare(right.name));
    return [
      ...(host
        ? [
            {
              value: "host" as const,
              label: "Host Tenant",
              icon: <HostTenantIcon size={14} aria-hidden="true" />,
            },
          ]
        : []),
      ...managed.map((tenant) => ({
        value: `managed:${tenant.name}` as const,
        label: tenant.display_name,
        summaryLabel: tenant.display_name,
        icon: <ManagedTenantIcon size={14} aria-hidden="true" />,
      })),
    ];
  }, [tenants]);
  const agentOptions = useMemo<SelectionOption<CodingAgentKind>[]>(
    () =>
      (["codex", "claude"] as const).map((value) => ({
        value,
        label: value === "codex" ? "Codex" : "Claude",
        icon: <BrandIcon brand={brandForAgent(value)} size={14} />,
      })),
    [],
  );
  const configTenantLabel =
    tenant.kind === "host"
      ? "Host Tenant"
      : (tenants.find((row) => row.kind === "managed" && row.name === tenant.name)?.display_name ??
        tenant.name);
  const configSelectionLabel = selection.current
    ? "Current Config"
    : `Named Config ${selection.config}`;
  const currentSelection = selection.current;
  const selectedTenantKey = configTenantKey(tenant);
  const selectedConfigKey = selection.current ? "current" : `named:${selection.config}`;
  const configFiles = catalog?.files ?? [];
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
  } = useConfigEditorSession(configFiles, `${selectedTenantKey}:${agent}`, onDirtyChange, setError);
  const paneRefs = useRef(new Map<string, HTMLDivElement>());
  useEffect(() => {
    visualModeInitialized.current = false;
    // A route-backed Config selection owns a distinct editor-mode lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setVisualAvailable(false);
    setEditorMode("raw");
  }, [agent, selectedConfigKey, selectedTenantKey]);
  useEffect(() => {
    if (!detailOpen || !file) return;
    const frame = window.requestAnimationFrame(() =>
      paneRefs.current.get(file)?.scrollIntoView?.({ block: "nearest" }),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, file, catalog]);
  useEffect(() => {
    // Loading a different external Config catalog resets editor-local state.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setVisualAvailable(false);
    setEditorMode("raw");
    setSelectionMode(false);
    setSelectedNames(new Set());
  }, [agent, selectedTenantKey]);
  const appliedName = catalog?.application.last_application?.applied ?? null;
  const selectedCount = selectedNames.size;
  const selectableNames = catalog?.configs.map((entry) => entry.name) ?? [];
  const allSelectable =
    selectableNames.length > 0 && selectableNames.every((name) => selectedNames.has(name));
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
  function selectTenant(values: ReadonlySet<TenantSelectionKey>) {
    const next = [...values][0];
    if (!next || next === configTenantKey(tenant)) return;
    requestEditorAction(() => {
      setTenant(tenantSelectionFromConfigKey(next));
      setSelection({ current: true });
      setSelectionMode(false);
      setSelectedNames(new Set());
      setDetailOpen(false);
      onLocationChange(configLocation(tenantSelectionFromConfigKey(next), agent, null));
    });
  }
  function selectAgent(values: ReadonlySet<CodingAgentKind>) {
    const next = [...values][0];
    if (!next || next === agent) return;
    requestEditorAction(() => {
      setAgent(next);
      setSelection({ current: true });
      setSelectionMode(false);
      setSelectedNames(new Set());
      setDetailOpen(false);
      onLocationChange(configLocation(tenant, next, null));
    });
  }
  function openConfig(name: string) {
    requestEditorAction(() => {
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      const nextSelection: ConfigSelection = { current: false, config: name };
      onLocationChange(configLocation(tenant, agent, nextSelection, file));
    });
  }
  function openCurrent() {
    requestEditorAction(() => {
      setSelection({ current: true });
      setDetailOpen(true);
      onLocationChange(configLocation(tenant, agent, { current: true }, file));
    });
  }
  function toggleConfig(name: string) {
    setSelectedNames((current) => {
      const next = new Set(current);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  }
  function toggleAllConfigs() {
    setSelectedNames(allSelectable ? new Set() : new Set(selectableNames));
  }
  function cancelSelection() {
    setSelectionMode(false);
    setSelectedNames(new Set());
  }
  function requestDelete(names: string[]) {
    if (names.length === 0) return;
    requestEditorAction(() => setDeleteTarget({ names }));
  }
  async function createConfig(name: string) {
    if (operationRunning || !name) return;
    setBusy(true);
    try {
      await api.createConfig(tenant, agent, name);
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadCatalog("background");
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      onLocationChange(configLocation(tenant, agent, { current: false, config: name }, file));
    } catch (cause) {
      setCreateError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }
  async function applyConfig(name: string) {
    if (operationRunning) return;
    setBusy(true);
    setApplyFeedback(null);
    let applyError: string | null = null;
    try {
      await api.applyConfig(tenant, agent, name);
    } catch (cause) {
      applyError = `${messageOf(cause)} Some Current Config files may already have been updated.`;
    } finally {
      const refreshed = await loadCatalog("background");
      if (refreshed && currentSelection) {
        reloadFiles(refreshed.files);
      }
      setApplyTarget(null);
      setError(applyError);
      if (!applyError) {
        setApplyFeedback(
          `Applied Named Config ${name} to Current Config. This is a one-time projection; it is not an Active Config.`,
        );
      }
      setBusy(false);
    }
  }
  async function deleteConfigs() {
    if (operationRunning || !deleteTarget || deleteTarget.names.length === 0) return;
    const requestedNames = deleteTarget.names;
    const wasSelectionMode = selectionMode;
    setBusy(true);
    try {
      await api.deleteConfigs(tenant, agent, requestedNames);
      const deletedSelected = !selection.current && requestedNames.includes(selection.config ?? "");
      setDeleteTarget(null);
      setSelectionMode(false);
      setSelectedNames(new Set());
      if (deletedSelected) {
        setSelection({ current: true });
        setDetailOpen(false);
        onLocationChange(configLocation(tenant, agent, null), true);
      }
      await loadCatalog("background");
    } catch (cause) {
      const deletionError = messageOf(cause);
      setDeleteTarget(null);
      const refreshed = await loadCatalog("background");
      if (refreshed) {
        const remaining = requestedNames.filter((name) =>
          refreshed.configs.some((entry) => entry.name === name),
        );
        setSelectedNames(wasSelectionMode ? new Set(remaining) : new Set());
        setSelectionMode(wasSelectionMode && remaining.length > 0);
        if (
          !selection.current &&
          !refreshed.configs.some((entry) => entry.name === selection.config)
        ) {
          setSelection({ current: true });
          setDetailOpen(false);
          onLocationChange(configLocation(tenant, agent, null), true);
        }
      }
      setError(deletionError);
    } finally {
      setBusy(false);
    }
  }
  async function previewPropagation() {
    setBusy(true);
    try {
      setPreview(await api.previewCredentialPropagation());
      setReport(null);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }
  async function executePropagation() {
    if (operationRunning || !preview) return;
    setBusy(true);
    try {
      setReport(await api.executeCredentialPropagation(preview.plan_id));
      setPreview(null);
      await loadCatalog("background");
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }
  const createNameValid = DNS_LABEL_PATTERN.test(newName);
  const propagationHasFailures =
    report?.entries.some((entry) => entry.outcome.status === "failed") ?? false;
  const propagationNeedsAttention =
    report?.entries.some((entry) => propagationGroup(entry.outcome.status) === "attention") ??
    false;
  return {
    agent,
    agentOptions,
    allSelectable,
    appliedName,
    applyConfig,
    applyFeedback,
    applyTarget,
    busy,
    cancelPending,
    cancelSelection,
    catalog,
    catalogError,
    configFiles,
    configRowButtons,
    configSelectionLabel,
    configTenantLabel,
    createConfig,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTitleId,
    deleteConfigs,
    deleteTarget,
    detailBackButtonRef,
    detailHeadingRef,
    detailOpen,
    dirtyFiles,
    discardAndRunPendingAction,
    editorMode,
    error,
    executePropagation,
    file,
    fileStatuses,
    handleLinkedFileSaved,
    handlePaneSaved,
    handleVisualAvailable,
    loadCatalog,
    loadingCatalog,
    loadingTenants,
    managedTenantMissing,
    mutationBusy,
    newName,
    openConfig,
    openCurrent,
    paneRefs,
    pendingAction,
    prepareMainConfigSave,
    preview,
    previewPropagation,
    propagationHasFailures,
    propagationNeedsAttention,
    propagationTitleId,
    refreshing,
    registerFileController,
    registerRevealRetry,
    report,
    requestDelete,
    requestEditorAction,
    retryReveals,
    retryTenants,
    saveInOrder,
    saveOrder,
    savePending,
    selectAgent,
    selectableNames,
    selectedCount,
    selectedNames,
    selection,
    selectionMode,
    selectTenant,
    setApplyTarget,
    setBusy,
    setCreateError,
    setCreateOpen,
    setDeleteTarget,
    setDetailOpen,
    setEditorMode,
    setError,
    setNewName,
    setPreview,
    setReport,
    setSelectionMode,
    switchEditorMode,
    tenant,
    tenantError,
    tenantOptions,
    toggleAllConfigs,
    toggleConfig,
    unsavedTitleId,
    visualAvailable,
  };
}
