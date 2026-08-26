import { flushSync } from "react-dom";
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  ListChecks,
  LoaderCircle,
  Plus,
  Trash2,
  Save,
} from "lucide-react";

import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import type { CodingAgentKind, TenantRow } from "@/api/core";

import type { Operation } from "@/api/operations";
import type { TenantSelection } from "@/api/tenantSelection";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { Dialog } from "@/shared/ui/Dialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { TextInput } from "@/shared/ui/FormControls";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu, type SelectionOption } from "@/shared/ui/SelectionMenu";
import { IssueIndicator } from "@/shared/ui/IssueIndicator";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { ConfigDriftBadge } from "@/features/configs/components/ConfigDriftBadge";
import {
  configIssueDescriptionId,
  configIssuePresentation,
  configWarningPresentation,
  propagationDetail,
  propagationGroup,
} from "@/features/configs/configCatalog";
import { ConfigFilePane } from "@/features/configs/editor/ConfigFilePane";
import type { ConfigFileController } from "@/features/configs/editor/configFileController";
import {
  configLocation,
  configTenantKey,
  readConfigRoute,
  tenantSelectionFromConfigKey,
  type ConfigApplyTarget,
  type ConfigDeleteTarget,
  type ConfigPendingAction,
  type ConfigSelection,
  type ConfigTenantKey,
} from "@/features/configs/route";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { DNS_LABEL_PATTERN } from "@/api/tenantSelection";
import { useTenants } from "@/shared/hooks/useTenants";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
interface PageProps {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange?: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}
export function ConfigPage({ api, operation, search, onDirtyChange, onLocationChange }: PageProps) {
  const [initialRoute] = useState(() => readConfigRoute(search));
  const observedSearch = useRef(search);
  const {
    tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useTenants(api);
  const [tenant, setTenant] = useState<TenantSelection>(initialRoute.tenant);
  const [agent, setAgent] = useState<CodingAgentKind>(initialRoute.agent);
  const [catalog, setCatalog] = useState<ConfigListData | null>(null);
  const [selection, setSelection] = useState<ConfigSelection>(initialRoute.selection);
  const selectionRef = useRef<ConfigSelection>(initialRoute.selection);
  const [file, setFile] = useState<string | null>(initialRoute.file);
  const [editorMode, setEditorMode] = useState<"visual" | "raw">("raw");
  const [visualAvailable, setVisualAvailable] = useState(false);
  const visualModeInitialized = useRef(false);
  const fileControllers = useRef(new Map<string, ConfigFileController>());
  const revealRetries = useRef(new Map<string, () => void>());
  const [fileStatuses, setFileStatuses] = useState<
    Record<
      string,
      {
        dirty: boolean;
        canSave: boolean;
      }
    >
  >({});
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedNames, setSelectedNames] = useState<Set<string>>(new Set());
  const [deleteTarget, setDeleteTarget] = useState<ConfigDeleteTarget | null>(null);
  const [applyTarget, setApplyTarget] = useState<ConfigApplyTarget | null>(null);
  const [applyFeedback, setApplyFeedback] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<ConfigPendingAction | null>(null);
  const [detailOpen, setDetailOpen] = useState(initialRoute.detailOpen);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const detailBackButtonRef = useRef<HTMLButtonElement>(null);
  const configRowButtons = useRef(new Map<string, HTMLButtonElement>());
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);
  const catalogController = useRef<AbortController | null>(null);
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
    onLocationChange?.(configLocation(tenant, agent, null), true);
  }, [agent, detailOpen, managedTenantMissing, onLocationChange, tenant]);
  const tenantOptions = useMemo<SelectionOption<ConfigTenantKey>[]>(() => {
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
  const loadCatalog = useCallback(
    async (kind: "initial" | "refresh" | "background" = "initial") => {
      catalogController.current?.abort();
      const controller = new AbortController();
      catalogController.current = controller;
      if (kind === "initial") setLoadingCatalog(true);
      if (kind === "refresh") setRefreshing(true);
      try {
        const data = await api.listConfigs(tenant, agent, controller.signal);
        if (controller.signal.aborted || catalogController.current !== controller) return null;
        const routedSelection = selectionRef.current;
        if (
          !routedSelection.current &&
          !data.configs.some((entry) => entry.name === routedSelection.config)
        ) {
          const fallback: ConfigSelection = { current: true };
          selectionRef.current = fallback;
          setSelection(fallback);
          setDetailOpen(false);
          onLocationChange?.(configLocation(tenant, agent, null), true);
        }
        setCatalog(data);
        setFile((current) =>
          current && data.files.includes(current) ? current : (data.files[0] ?? null),
        );
        setSelectedNames(
          (current) =>
            new Set(
              [...current].filter((name) => data.configs.some((entry) => entry.name === name)),
            ),
        );
        setError(null);
        return data;
      } catch (cause) {
        if (!(controller.signal.aborted || cause instanceof DOMException))
          setError(messageOf(cause));
        return null;
      } finally {
        if (catalogController.current === controller) {
          catalogController.current = null;
          if (kind === "initial") setLoadingCatalog(false);
          if (kind === "refresh") setRefreshing(false);
        }
      }
    },
    [agent, api, onLocationChange, tenant],
  );
  useEffect(() => {
    // Loading a different external Config catalog replaces the previous catalog lifecycle.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setCatalog(null);
    fileControllers.current.clear();
    setFileStatuses({});
    setVisualAvailable(false);
    setEditorMode("raw");
    setSelectionMode(false);
    setSelectedNames(new Set());
    void loadCatalog();
    return () => catalogController.current?.abort();
  }, [loadCatalog]);
  const appliedName = catalog?.application.last_application?.applied ?? null;
  const selectedCount = selectedNames.size;
  const selectableNames = catalog?.configs.map((entry) => entry.name) ?? [];
  const allSelectable =
    selectableNames.length > 0 && selectableNames.every((name) => selectedNames.has(name));
  const editorDirty = Object.values(fileStatuses).some((status) => status.dirty);
  const dirtyFiles = (catalog?.files ?? []).filter((name) => fileStatuses[name]?.dirty);
  useEffect(() => onDirtyChange?.(editorDirty), [editorDirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  const registerFileController = useCallback(
    (name: string, controller: ConfigFileController | null) => {
      setFileStatuses((current) => {
        const next = { ...current };
        if (controller) {
          fileControllers.current.set(name, controller);
          next[name] = { dirty: controller.dirty, canSave: controller.canSave };
        } else {
          fileControllers.current.delete(name);
          delete next[name];
        }
        return next;
      });
    },
    [],
  );
  const registerRevealRetry = useCallback((name: string, retry: (() => void) | null) => {
    if (retry) revealRetries.current.set(name, retry);
    else revealRetries.current.delete(name);
  }, []);
  const handlePaneSaved = useCallback(() => {
    void loadCatalog("background");
  }, [loadCatalog]);
  const prepareMainConfigSave = useCallback((customProvider: boolean) => {
    if (!customProvider) return true;
    const auth = fileControllers.current.get("auth.json");
    if (!auth?.dirty) return true;
    setError("Save auth.json before saving a Custom provider configuration.");
    return false;
  }, []);
  const handleLinkedFileSaved = useCallback((name: string) => {
    fileControllers.current.get(name)?.reload();
  }, []);
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
  const requestEditorAction = useCallback(
    (run: () => void | Promise<void>) => {
      if (editorDirty) setPendingAction({ run });
      else void run();
    },
    [editorDirty],
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
  async function saveAndRunPendingAction() {
    if (!pendingAction) return;
    const action = pendingAction.run;
    const names = agent === "codex" ? ["auth.json", "config.toml"] : (catalog?.files ?? []);
    for (const name of names) {
      const controller = fileControllers.current.get(name);
      if (controller?.dirty && !(await controller.save())) return;
    }
    setPendingAction(null);
    await action();
  }
  async function discardAndRunPendingAction() {
    if (!pendingAction) return;
    const action = pendingAction.run;
    for (const controller of fileControllers.current.values()) {
      if (controller.dirty) controller.restore();
    }
    setPendingAction(null);
    await action();
  }
  function selectTenant(values: ReadonlySet<ConfigTenantKey>) {
    const next = [...values][0];
    if (!next || next === configTenantKey(tenant)) return;
    requestEditorAction(() => {
      setTenant(tenantSelectionFromConfigKey(next));
      setSelection({ current: true });
      setSelectionMode(false);
      setSelectedNames(new Set());
      setDetailOpen(false);
      onLocationChange?.(configLocation(tenantSelectionFromConfigKey(next), agent, null));
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
      onLocationChange?.(configLocation(tenant, next, null));
    });
  }
  function openConfig(name: string) {
    requestEditorAction(() => {
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      const nextSelection: ConfigSelection = { current: false, config: name };
      onLocationChange?.(configLocation(tenant, agent, nextSelection, file));
    });
  }
  function openCurrent() {
    requestEditorAction(() => {
      setSelection({ current: true });
      setDetailOpen(true);
      onLocationChange?.(configLocation(tenant, agent, { current: true }, file));
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
      onLocationChange?.(configLocation(tenant, agent, { current: false, config: name }, file));
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
        for (const currentFile of refreshed.files) {
          fileControllers.current.get(currentFile)?.reload();
        }
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
        onLocationChange?.(configLocation(tenant, agent, null), true);
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
          onLocationChange?.(configLocation(tenant, agent, null), true);
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
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={
          tenantError
            ? retryTenants
            : error
              ? () => {
                  setError(null);
                  for (const retry of revealRetries.current.values()) retry();
                  void loadCatalog("refresh");
                }
              : undefined
        }
      />
      <MutationUnavailable operation={operation} />
      <div className={`${layout.splitLayout} ${detailOpen ? layout.showsDetail : ""}`}>
        <aside className={styles.configCatalog} aria-label="Configs">
          <div className={layout.toolbar}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={layout.selectionCancel}
                  disabled={busy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={layout.toolbarSelectionActions}>
                  <span className={layout.selectionCount}>{selectedCount} selected</span>
                  <button
                    type="button"
                    className={layout.selectionAll}
                    disabled={selectableNames.length === 0 || busy}
                    onClick={toggleAllConfigs}
                  >
                    {allSelectable ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={layout.selectionDelete}
                    aria-label="Delete selected Named Configs"
                    disabled={selectedCount === 0 || mutationBusy}
                    onClick={() => requestDelete([...selectedNames])}
                  >
                    <Trash2 size={14} aria-hidden="true" /> Delete selected
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className={layout.toolbarFilters}>
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Tenant"
                    onCommit={selectTenant}
                    options={tenantOptions}
                    pluralLabel="tenants"
                    selected={new Set([configTenantKey(tenant)])}
                    triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                    unavailableSummary={
                      loadingTenants
                        ? "Loading"
                        : managedTenantMissing
                          ? "Not found"
                          : "Unavailable"
                    }
                    allowMultiple={false}
                  />
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Coding Agent"
                    onCommit={selectAgent}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={new Set([agent])}
                    triggerIcon={<BrandIcon brand={brandForAgent(agent)} size={14} />}
                    allowMultiple={false}
                  />
                </div>
                <div className={layout.toolbarActions}>
                  <RefreshButton
                    className={layout.refreshAction}
                    label="Refresh Configs"
                    busyLabel="Refreshing Configs"
                    busy={refreshing}
                    disabled={loadingCatalog || refreshing || busy}
                    onClick={() =>
                      requestEditorAction(async () => {
                        await loadCatalog("refresh");
                      })
                    }
                  >
                    Refresh
                  </RefreshButton>
                  <button
                    type="button"
                    className={layout.selectionEnter}
                    aria-label="Select Configs"
                    disabled={selectableNames.length === 0 || loadingCatalog || refreshing || busy}
                    onClick={() => setSelectionMode(true)}
                  >
                    <ListChecks size={14} /> Select
                  </button>
                </div>
              </>
            )}
          </div>
          <div className={styles.configWarnings} aria-live="polite">
            {appliedName && !applyFeedback && (
              <div className={styles.applicationNotice}>
                <Check size={15} aria-hidden="true" />
                <span>
                  Last applied: <strong>Named Config {appliedName}</strong>. Application is a
                  one-time projection to Current Config, not an Active Config.
                </span>
              </div>
            )}
            {applyFeedback && (
              <div className={styles.applicationNotice} role="status">
                <Check size={15} aria-hidden="true" />
                <span>{applyFeedback}</span>
              </div>
            )}
            {catalog?.application.drift === "source-missing" && (
              <div className={styles.inlineWarning}>
                <AlertTriangle size={15} />
                <span title={catalog.application.detail}>
                  Last applied Named Config is missing.
                </span>
              </div>
            )}
            {catalog?.application.drift === "comparison-error" && catalog.application.detail && (
              <div className={styles.inlineWarning}>
                <AlertTriangle size={15} />
                <span>{catalog.application.detail}</span>
              </div>
            )}
          </div>
          <div className={layout.list} aria-busy={loadingCatalog}>
            {(loadingTenants || loadingCatalog) && !catalog && <Loading />}
            <div className={layout.rowGroup}>
              {!managedTenantMissing && (
                <div
                  className={`${layout.row} ${selection.current ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
                >
                  <button
                    ref={(element) => {
                      if (element) configRowButtons.current.set("current", element);
                      else configRowButtons.current.delete("current");
                    }}
                    type="button"
                    className={styles.configRowMain}
                    aria-label={
                      selectionMode ? "Current Config cannot be selected" : "Current Config"
                    }
                    aria-pressed={!selectionMode && selection.current ? true : undefined}
                    disabled={busy || loadingCatalog || (selectionMode ? true : false)}
                    onClick={() => void openCurrent()}
                  >
                    <CurrentConfigIcon size={16} data-icon="current-config" />
                    <span className={styles.configRowText}>
                      <strong>Current Config</strong>
                    </span>
                    {selectionMode && <span className={layout.protectedBadge}>Protected</span>}
                  </button>
                  {!selectionMode &&
                    tenant.kind === "host" &&
                    agent === "codex" &&
                    catalog?.credential_propagation_available && (
                      <button
                        type="button"
                        className={`${styles.configRowPrimaryAction} ${styles.configPropagateAction}`}
                        title="Propagate credentials"
                        aria-label="Propagate credentials"
                        disabled={mutationBusy}
                        onClick={() => void previewPropagation()}
                      >
                        Propagate credentials
                      </button>
                    )}
                </div>
              )}
              <div className={layout.divider}>
                <span>Named Configs</span>
                <IconButton
                  className={layout.addAction}
                  label="Create Named Config"
                  disabled={mutationBusy || loadingCatalog || selectionMode}
                  onClick={() =>
                    requestEditorAction(() => {
                      setCreateError(null);
                      setCreateOpen(true);
                    })
                  }
                >
                  <Plus size={15} />
                </IconButton>
              </div>
              {catalog?.configs.map((entry) => {
                const applied = entry.name === appliedName;
                const selectedForDeletion = selectedNames.has(entry.name);
                const selectedForInspection = !selection.current && selection.config === entry.name;
                const issue = configIssuePresentation(entry) ?? configWarningPresentation(entry);
                const issueDescriptionId = issue
                  ? configIssueDescriptionId(tenant, agent, entry.name)
                  : undefined;
                return (
                  <div
                    key={entry.name}
                    className={`${layout.row} ${selectedForInspection ? layout.rowInspected : ""} ${selectedForDeletion ? layout.rowSelected : ""} ${selectionMode ? layout.rowSelectable : ""}`}
                  >
                    <button
                      ref={(element) => {
                        if (element) configRowButtons.current.set(entry.name, element);
                        else configRowButtons.current.delete(entry.name);
                      }}
                      type="button"
                      className={styles.configRowMain}
                      aria-label={
                        selectionMode
                          ? `${selectedForDeletion ? "Deselect" : "Select"} ${entry.name}`
                          : entry.name
                      }
                      aria-describedby={issueDescriptionId}
                      aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                      disabled={busy || loadingCatalog}
                      onClick={() =>
                        selectionMode ? toggleConfig(entry.name) : void openConfig(entry.name)
                      }
                    >
                      <NamedConfigIcon size={16} />
                      <span className={styles.configRowText}>
                        <span className={styles.configRowTitle}>
                          <strong>{entry.name}</strong>
                          {issue && (
                            <IssueIndicator
                              tone={issue.tone}
                              label={issue.label}
                              message={issue.message}
                              ariaLabel={issue.accessibleLabel}
                            />
                          )}
                          {applied && <ConfigDriftBadge status={catalog.application} />}
                        </span>
                      </span>
                      {selectionMode && (
                        <span className={layout.selectionIndicator} aria-hidden="true">
                          {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                        </span>
                      )}
                      {issue && (
                        <span id={issueDescriptionId} className="srOnly">
                          {issue.accessibleLabel}
                        </span>
                      )}
                    </button>
                    {!selectionMode && (
                      <div className={layout.rowActions}>
                        {entry.state === "ready" && (
                          <button
                            type="button"
                            className={styles.configRowPrimaryAction}
                            title={
                              applied && catalog.application.drift === "clean"
                                ? "Already clean"
                                : `Apply Named Config ${entry.name} to Current Config`
                            }
                            aria-label={`Apply Named Config ${entry.name} to Current Config`}
                            disabled={
                              mutationBusy || (applied && catalog.application.drift === "clean")
                            }
                            onClick={() =>
                              requestEditorAction(() => setApplyTarget({ name: entry.name }))
                            }
                          >
                            Apply to Current Config
                          </button>
                        )}
                        {entry.state === "incomplete" && (
                          <button
                            type="button"
                            className={styles.configRowPrimaryAction}
                            title={`Repair Named Config ${entry.name}`}
                            aria-label={`Repair Named Config ${entry.name}`}
                            disabled={mutationBusy}
                            onClick={() => requestEditorAction(() => createConfig(entry.name))}
                          >
                            Repair
                          </button>
                        )}
                        <IconButton
                          className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                          label={`Delete Named Config ${entry.name}`}
                          disabled={mutationBusy}
                          onClick={() => requestDelete([entry.name])}
                        >
                          <Trash2 size={15} />
                        </IconButton>
                      </div>
                    )}
                  </div>
                );
              })}
              {catalog && catalog.configs.length === 0 && !loadingCatalog && (
                <EmptyState
                  variant="list"
                  icon={<NamedConfigIcon size={22} aria-hidden="true" />}
                  title="No Named Configs found."
                />
              )}
            </div>
          </div>
        </aside>
        <section className={layout.detailPane}>
          {loadingTenants || loadingCatalog ? (
            <Loading />
          ) : managedTenantMissing ? (
            <EmptyState
              variant="detail"
              icon={<ManagedTenantIcon size={26} aria-hidden="true" />}
              title="Managed Tenant not found"
              description="The selected Managed Tenant does not exist."
            />
          ) : catalog ? (
            <>
              <div className={styles.configEditorHeader}>
                <IconButton
                  buttonRef={detailBackButtonRef}
                  label="Back to Configs"
                  onClick={() =>
                    requestEditorAction(() => {
                      const focusKey = selection.current ? "current" : selection.config;
                      flushSync(() => setDetailOpen(false));
                      if (focusKey) {
                        const target = configRowButtons.current.get(focusKey);
                        target?.focus();
                      }
                      onLocationChange?.(configLocation(tenant, agent, null));
                    })
                  }
                >
                  <ChevronLeft size={17} />
                </IconButton>
                <div className={styles.configContextStack}>
                  <div className={styles.contextFacts} aria-label="Config editing context">
                    <span>
                      <small>Tenant</small>
                      <strong>
                        {configTenantLabel}
                        {tenant.kind === "host" && <em>Host risk</em>}
                      </strong>
                    </span>
                    <span>
                      <small>Coding Agent</small>
                      <strong>{agent === "codex" ? "Codex" : "Claude"}</strong>
                    </span>
                    <span>
                      <small>Config</small>
                      <strong>{configSelectionLabel}</strong>
                    </span>
                    <span>
                      <small>File</small>
                      <strong
                        className={styles.contextFile}
                        title={agent === "codex" ? "config.toml + auth.json" : "settings.json"}
                      >
                        {agent === "codex" ? "config.toml + auth.json" : "settings.json"}
                      </strong>
                    </span>
                  </div>
                  {(selection.current || agent === "codex" || editorMode === "raw") && (
                    <span className={styles.sensitiveContext}>
                      Native content may contain credentials and is displayed without redaction.
                    </span>
                  )}
                  <h2 ref={detailHeadingRef} tabIndex={-1}>
                    {agent === "codex" && !selection.current
                      ? "Codex configuration"
                      : (file ?? "Configuration")}
                  </h2>
                </div>
              </div>
              <div className={styles.configFilePanel}>
                <div className={styles.editorModeBar} aria-label="Editor mode">
                  <span>
                    {dirtyFiles.length > 0
                      ? `${dirtyFiles.length} unsaved file${dirtyFiles.length === 1 ? "" : "s"}`
                      : "All files saved"}
                  </span>
                  <div className={styles.segmented}>
                    {visualAvailable && !selection.current && (
                      <button
                        type="button"
                        aria-pressed={editorMode === "visual"}
                        onClick={() => switchEditorMode("visual")}
                      >
                        Visual
                      </button>
                    )}
                    <button
                      type="button"
                      aria-pressed={editorMode === "raw"}
                      onClick={() => switchEditorMode("raw")}
                    >
                      Raw
                    </button>
                  </div>
                  {dirtyFiles.length > 0 && (
                    <ActionButton
                      tone="primary"
                      disabled={mutationBusy}
                      onClick={() => {
                        void (async () => {
                          setBusy(true);
                          const names =
                            agent === "codex" ? ["auth.json", "config.toml"] : configFiles;
                          for (const name of names) {
                            const controller = fileControllers.current.get(name);
                            if (controller?.dirty && !(await controller.save())) break;
                          }
                          await loadCatalog("background");
                          setBusy(false);
                        })();
                      }}
                    >
                      <Save size={14} /> Save all
                    </ActionButton>
                  )}
                </div>
                <div className={styles.configFileStack}>
                  {configFiles.map((name) => (
                    <div
                      key={name}
                      ref={(element) => {
                        if (element) paneRefs.current.set(name, element);
                        else paneRefs.current.delete(name);
                      }}
                      className={`${styles.configFileSection} ${file === name ? styles.configFileSectionFocused : ""}`}
                    >
                      <ConfigFilePane
                        key={`${configTenantKey(tenant)}:${agent}:${selection.current ? "current" : `named:${selection.config}`}:${name}`}
                        api={api}
                        tenant={tenant}
                        agent={agent}
                        selection={selection}
                        file={name}
                        mode={selection.current ? "raw" : editorMode}
                        operationBusy={mutationBusy}
                        onControllerChange={registerFileController}
                        onError={setError}
                        onRevealRetryChange={registerRevealRetry}
                        onSaved={handlePaneSaved}
                        onBeforeSave={name === "config.toml" ? prepareMainConfigSave : undefined}
                        onLinkedFileSaved={handleLinkedFileSaved}
                        onVisualAvailable={
                          name === (agent === "claude" ? "settings.json" : "config.toml")
                            ? handleVisualAvailable
                            : undefined
                        }
                        onRequestRaw={() => setEditorMode("raw")}
                      />
                    </div>
                  ))}
                </div>
              </div>
            </>
          ) : (
            <div className={styles.emptyPane} role="status">
              <AlertTriangle size={22} aria-hidden="true" />
              <span>Configuration is unavailable. Use Retry to load it again.</span>
            </div>
          )}
        </section>
      </div>
      {pendingAction && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={unsavedTitleId}
          busy={mutationBusy}
          onCancel={() => setPendingAction(null)}
        >
          <section>
            <h2 id={unsavedTitleId}>Unsaved changes</h2>
            <p>
              Save changes to{" "}
              {dirtyFiles.length > 1
                ? `${dirtyFiles.length} files`
                : (dirtyFiles[0] ?? "this file")}{" "}
              before continuing?
            </p>
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setPendingAction(null)} disabled={busy}>
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void discardAndRunPendingAction()}
                disabled={busy}
              >
                Discard and continue
              </button>
              <ActionButton
                tone="primary"
                onClick={() => void saveAndRunPendingAction()}
                disabled={mutationBusy || dirtyFiles.some((name) => !fileStatuses[name]?.canSave)}
              >
                Save and continue
              </ActionButton>
            </div>
          </section>
        </Dialog>
      )}
      {createOpen && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={createTitleId}
          busy={mutationBusy}
          onCancel={() => setCreateOpen(false)}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (createNameValid && !mutationBusy) void createConfig(newName);
            }}
          >
            <h2 id={createTitleId}>Create Named Config</h2>
            <label>
              Name
              <TextInput
                autoFocus
                aria-label="Named Config name"
                value={newName}
                onChange={(event) => {
                  setNewName(event.target.value);
                  setCreateError(null);
                }}
                aria-invalid={newName.length > 0 && !createNameValid}
                aria-describedby={createHelpId}
              />
            </label>
            <p id={createHelpId} className={layout.dialogDescription}>
              Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or
              number.
            </p>
            {newName.length > 0 && !createNameValid && (
              <div className={styles.inlineWarning} role="alert">
                Enter a valid lowercase DNS label.
              </div>
            )}
            {createError && <div className={styles.inlineWarning}>{createError}</div>}
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setCreateOpen(false)} disabled={busy}>
                Cancel
              </button>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!createNameValid || mutationBusy}
              >
                {busy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Plus size={14} />
                )}
                {busy ? "Creating…" : "Create"}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {applyTarget && (
        <ConfirmDialog
          title={`Apply Named Config ${applyTarget.name} to Current Config?`}
          description={
            <div className={layout.dialogDescription}>
              <p>
                Tenant: <strong>{configTenantLabel}</strong>
                <br />
                Coding Agent: <strong>{agent === "codex" ? "Codex" : "Claude"}</strong>
                <br />
                Source: <strong>Named Config {applyTarget.name}</strong>
                <br />
                Target: <strong>Current Config</strong>
              </p>
              <p>
                Included fixed Config Fields may be added or replaced; omitted fixed fields are
                removed. Unrelated native configuration is preserved. This is a one-time projection
                to Current Config and does not create an Active Config. Files commit one at a time;
                a later file failure does not roll back earlier updates.
              </p>
            </div>
          }
          confirmation={tenant.kind === "host" ? "Host Tenant" : undefined}
          confirmLabel="Apply to Current Config"
          variant="primary"
          busy={mutationBusy}
          onCancel={() => setApplyTarget(null)}
          onConfirm={() => void applyConfig(applyTarget.name)}
        />
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Named Config ${deleteTarget.names[0]}?`}
          description={
            <p className={layout.dialogDescription}>
              This deletes only the Named Config. Current Config stays unchanged; if this was the
              last applied source, Config Drift will report it as missing.
            </p>
          }
          confirmLabel="Delete Config"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Named Configs?"
          description={
            <>
              <p className={layout.dialogDescription}>
                This deletes only the selected Named Configs. Current Config files are not changed.
                If a last applied source is deleted, Config Drift becomes Source missing.
              </p>
              <div className={styles.planList}>
                {deleteTarget.names.map((name) => (
                  <code key={name}>{name}</code>
                ))}
              </div>
            </>
          }
          confirmLabel="Delete selected"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {(preview || report) && (
        <Dialog
          className={`${layout.dialog} ${styles.wideDialog}`}
          ariaLabelledBy={propagationTitleId}
          busy={mutationBusy}
          onCancel={() => {
            setPreview(null);
            setReport(null);
          }}
        >
          <section>
            <h2 id={propagationTitleId}>
              {preview ? "Credential Propagation preview" : "Credential Propagation result"}
            </h2>
            {report && (
              <div
                className={`${styles.propagationSummary} ${
                  propagationHasFailures || propagationNeedsAttention
                    ? styles.propagationSummaryPartial
                    : styles.propagationSummaryComplete
                }`}
                role={propagationHasFailures ? "alert" : "status"}
              >
                {propagationHasFailures
                  ? "Partially completed. Successful credential updates were kept; failed targets need attention."
                  : propagationNeedsAttention
                    ? "Credential propagation completed with targets that need attention."
                    : "Credential propagation completed."}
              </div>
            )}
            <div className={styles.propagationGroups}>
              {(["updated", "skipped", "attention"] as const).map((group) => {
                const entries = (preview?.preview.entries ?? report?.entries ?? []).filter(
                  (entry) => propagationGroup(entry.outcome.status) === group,
                );
                if (entries.length === 0) return null;
                const heading =
                  group === "updated"
                    ? "Updated"
                    : group === "skipped"
                      ? "Skipped"
                      : "Needs attention";
                return (
                  <section key={group}>
                    <h3>
                      {heading} <span>{entries.length}</span>
                    </h3>
                    <div className={styles.planList}>
                      {entries.map((entry) => (
                        <div key={entry.label}>
                          <code>{entry.label}</code>
                          <span>
                            {preview && entry.outcome.status === "updated"
                              ? "Will update"
                              : entry.outcome.status}
                            {propagationDetail(entry.outcome) && (
                              <small>{propagationDetail(entry.outcome)}</small>
                            )}
                          </span>
                        </div>
                      ))}
                    </div>
                  </section>
                );
              })}
              {(preview?.preview.entries.length ?? report?.entries.length ?? 0) === 0 && (
                <p>No matching credentials.</p>
              )}
            </div>
            <div className={styles.dialogActions}>
              <button
                type="button"
                onClick={() => {
                  setPreview(null);
                  setReport(null);
                }}
              >
                Close
              </button>
              {preview && (
                <ActionButton
                  tone="primary"
                  disabled={mutationBusy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  {busy && <LoaderCircle className="spin" size={14} aria-hidden="true" />}
                  {busy
                    ? "Propagating…"
                    : `Propagate ${preview.preview.updates} credential update${preview.preview.updates === 1 ? "" : "s"}`}
                </ActionButton>
              )}
            </div>
          </section>
        </Dialog>
      )}
    </div>
  );
}
