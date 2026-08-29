import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type { ComponentKind, ComponentRow, TenantApi } from "@/api/tenants";
import {
  abbreviateTenantHome,
  COMPONENT_GROUPS,
  compareStableVersions,
  componentMenuCoordinates,
  componentProgressLabel,
  hasComponentAttention,
  latestEntryFor,
  tenantSelection,
} from "@/features/tenants/componentCatalog";
import { fallbackTenantKey, tenantKeyOf, tenantLocation } from "@/features/tenants/route";
import { useComponentCatalog } from "@/features/tenants/useComponentCatalog";
import { useTenantCatalog } from "@/features/tenants/useTenantCatalog";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import {
  DNS_LABEL_PATTERN,
  parseTenantSelectionKey,
  type TenantSelectionKey,
} from "@/domain/tenant";

type TenantDeleteTarget = { names: string[] };
type ComponentRemoveTarget = { row: ComponentRow; tenantLabel: string };
type ComponentSpecificVersionTarget = {
  row: ComponentRow;
  tenantLabel: string;
  mode: "install" | "update";
};
type ComponentActionProgress = {
  tenantKey: TenantSelectionKey;
  kind: ComponentKind;
  label: string;
};

interface ControllerOptions {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export function useTenantController({
  api,
  operation,
  search,
  onLocationChange,
  onOperation,
}: ControllerOptions) {
  const [initialRoute] = useState(() => new URLSearchParams(search));
  const observedSearch = useRef<string | null>(null);
  const normalizedComponentSearch = useRef<string | null>(null);
  const initialKey = parseTenantSelectionKey(initialRoute.get("tenant"));
  const {
    tenants,
    loading: loadingTenants,
    error: tenantCatalogError,
    load: loadTenants,
  } = useTenantCatalog(api);
  const [selectedKey, setSelectedKey] = useState<string | null>(initialKey);
  const [componentActionProgress, setComponentActionProgress] =
    useState<ComponentActionProgress | null>(null);
  const [expandedComponents, setExpandedComponents] = useState<Set<string>>(new Set());
  const [openComponentMenu, setOpenComponentMenu] = useState<ComponentKind | null>(null);
  const [componentMenuPosition, setComponentMenuPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const [specificVersionTarget, setSpecificVersionTarget] =
    useState<ComponentSpecificVersionTarget | null>(null);
  const [specificVersion, setSpecificVersion] = useState("");
  const [specificVersionError, setSpecificVersionError] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<TenantSelectionKey>>(new Set());
  const [deleteTarget, setDeleteTarget] = useState<TenantDeleteTarget | null>(null);
  const [componentRemoveTarget, setComponentRemoveTarget] = useState<ComponentRemoveTarget | null>(
    null,
  );
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(initialKey !== null);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const tenantRowButtons = useRef(new Map<string, HTMLButtonElement>());
  const componentMenuButtons = useRef(new Map<string, HTMLButtonElement>());
  const componentMenuItems = useRef(new Map<string, HTMLButtonElement>());
  const componentMenuRef = useRef<HTMLDivElement>(null);
  const refreshedOperation = useRef<string | null>(null);
  const createTitleId = useId();
  const createHelpId = useId();
  const specificVersionTitleId = useId();
  const specificVersionHelpId = useId();
  const [copiedHome, copyHome] = useClipboardFeedback<string>();
  const {
    checkingLatest,
    components,
    latestSnapshot,
    load: loadComponentCatalog,
    loading: loadingComponents,
    preserveNextError: preserveNextComponentError,
    checkLatest,
    tenantKey: componentsTenantKey,
  } = useComponentCatalog(api, setError);
  const selected = tenants.find((row) => tenantKeyOf(row) === selectedKey) ?? null;
  const hostTenant = tenants.find((row) => row.kind === "host") ?? null;
  const managedTenants = useMemo(
    () =>
      tenants
        .filter(
          (
            row,
          ): row is TenantRow & {
            kind: "managed";
            name: string;
          } => row.kind === "managed" && Boolean(row.name),
        )
        .sort((left, right) => left.name.localeCompare(right.name)),
    [tenants],
  );
  const selectableKeys = managedTenants
    .filter((row) => row.name !== "default")
    .map((row) => tenantKeyOf(row));
  const allSelectable =
    selectableKeys.length > 0 && selectableKeys.every((key) => selectedKeys.has(key));
  const selectedCount = selectedKeys.size;
  const createNameValid = DNS_LABEL_PATTERN.test(newName);
  const mutationBusy = busy || operation?.state === "running" || componentActionProgress !== null;
  const componentCatalogLoading =
    loadingComponents || (selectedKey !== null && componentsTenantKey !== selectedKey);
  const visibleComponents = componentCatalogLoading ? [] : components;
  const componentTotalCount = selected?.kind === "host" ? 2 : 8;
  const installedComponentCount = visibleComponents.filter(
    (row) => row.status === "installed" || row.status === "modified",
  ).length;
  const attentionComponentCount = visibleComponents.filter((row) =>
    hasComponentAttention(row, latestSnapshot),
  ).length;
  const componentGroups = COMPONENT_GROUPS.map((group) => ({
    ...group,
    rows: group.kinds
      .map((kind) => visibleComponents.find((row) => row.kind === kind))
      .filter((row): row is ComponentRow => Boolean(row)),
  })).filter((group) => group.rows.length > 0);
  const selectedHome = selected
    ? abbreviateTenantHome(selected.home, hostTenant?.home ?? null)
    : "";
  const tenantKindLabel = selected?.kind === "host" ? "Host Tenant" : "Managed Tenant";
  const specificVersionValue = specificVersion.trim();
  const specificVersionFormatValid = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(
    specificVersionValue,
  );
  let specificVersionValidationError: string | null = null;
  if (specificVersion.length > 0 && !specificVersionFormatValid) {
    specificVersionValidationError = "Enter a stable version in X.Y.Z form.";
  } else if (specificVersionFormatValid && specificVersionTarget?.mode === "update") {
    const currentVersion = specificVersionTarget.row.version;
    const currentComparison = currentVersion
      ? compareStableVersions(specificVersionValue, currentVersion)
      : null;
    if (currentComparison === 0) {
      specificVersionValidationError = `Version v${currentVersion} is already installed.`;
    } else if (currentComparison === -1) {
      specificVersionValidationError = `Enter a version newer than v${currentVersion}. Remove the Component before installing a lower version.`;
    } else if (currentComparison === null) {
      specificVersionValidationError = "The installed version cannot be compared safely.";
    }
  }
  const specificVersionValid = specificVersionFormatValid && !specificVersionValidationError;
  useEffect(() => {
    const query = new URLSearchParams(search);
    if (query.has("component") && normalizedComponentSearch.current !== search) {
      query.delete("component");
      normalizedComponentSearch.current = search;
      onLocationChange(query, true);
    } else if (!query.has("component")) {
      normalizedComponentSearch.current = null;
    }
    if (observedSearch.current === search) return;
    observedSearch.current = search;
    const key = parseTenantSelectionKey(query.get("tenant"));
    setSelectedKey(key ?? fallbackTenantKey(tenants));
    setDetailOpen(key !== null);
  }, [onLocationChange, search, tenants]);
  useEffect(() => {
    if (!openComponentMenu) return;
    function positionMenu() {
      if (!openComponentMenu) return;
      const button = componentMenuButtons.current.get(openComponentMenu);
      const menu = componentMenuRef.current;
      if (!button || !menu) return;
      const menuBounds = menu.getBoundingClientRect();
      setComponentMenuPosition(
        componentMenuCoordinates(
          button.getBoundingClientRect(),
          menuBounds.width,
          menuBounds.height,
        ),
      );
    }
    const positionFrame = window.requestAnimationFrame(positionMenu);
    const focusFrame = window.requestAnimationFrame(() => {
      positionMenu();
      componentMenuItems.current.get(openComponentMenu)?.focus();
    });
    const closeOnPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      const button = componentMenuButtons.current.get(openComponentMenu);
      if (button?.contains(target) || componentMenuRef.current?.contains(target)) return;
      setOpenComponentMenu(null);
    };
    const closeOnKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      const key = openComponentMenu;
      setOpenComponentMenu(null);
      window.requestAnimationFrame(() => componentMenuButtons.current.get(key)?.focus());
    };
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnKeyDown);
    window.addEventListener("resize", positionMenu);
    window.addEventListener("scroll", positionMenu, true);
    return () => {
      window.cancelAnimationFrame(positionFrame);
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnKeyDown);
      window.removeEventListener("resize", positionMenu);
      window.removeEventListener("scroll", positionMenu, true);
    };
  }, [openComponentMenu]);
  useEffect(() => {
    if (!detailOpen || !selectedKey || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() => detailHeadingRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, selectedKey]);
  useEffect(() => {
    if (loadingTenants) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSelectedKey((current) => {
      if (current && tenants.some((row) => tenantKeyOf(row) === current)) return current;
      const fallback = fallbackTenantKey(tenants);
      if (current) {
        setDetailOpen(false);
        onLocationChange(new URLSearchParams(), true);
      }
      return fallback;
    });
    setSelectedKeys((current) => {
      const available = new Set(tenants.map((row) => tenantKeyOf(row)));
      return new Set(
        [...current].filter(
          (key) => available.has(key) && key !== "host" && key !== "managed:default",
        ),
      );
    });
  }, [loadingTenants, onLocationChange, tenants]);
  const loadComponents = useCallback(
    async (target: TenantRow | null, showLoading = false) => {
      if (showLoading) {
        setExpandedComponents(new Set());
        setOpenComponentMenu(null);
      }
      const rows = await loadComponentCatalog(target, showLoading);
      if (rows) {
        setExpandedComponents(new Set());
        setOpenComponentMenu(null);
      }
    },
    [loadComponentCatalog],
  );
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadComponents(selected, true);
  }, [loadComponents, selected]);
  useEffect(() => {
    if (!operation || operation.state === "running" || refreshedOperation.current === operation.id)
      return;
    refreshedOperation.current = operation.id;
    setComponentActionProgress(null);
    void loadTenants();
    void loadComponents(selected);
  }, [loadComponents, loadTenants, operation, selected]);

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
    if (rows) await loadComponents(selected, true);
  }

  async function checkForUpdates() {
    if (checkingLatest) return;
    await Promise.all([checkLatest(), loadComponents(selected)]);
  }

  async function createTenant() {
    if (!createNameValid) return;
    setBusy(true);
    try {
      await api.createTenant(newName);
      const created = newName;
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadTenants();
      const key = `managed:${created}` as TenantSelectionKey;
      setSelectedKey(key);
      setDetailOpen(true);
      onLocationChange(tenantLocation(key));
    } catch (cause) {
      setCreateError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  function toggleTenant(key: TenantSelectionKey) {
    if (key === "host" || key === "managed:default") return;
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  function toggleAllTenants() {
    setSelectedKeys(allSelectable ? new Set() : new Set(selectableKeys));
  }

  function cancelSelection() {
    setSelectionMode(false);
    setSelectedKeys(new Set());
  }

  function requestTenantDelete(names: string[]) {
    if (names.length === 0) return;
    setDeleteTarget({ names });
  }

  async function deleteTenants() {
    if (!deleteTarget || deleteTarget.names.length === 0) return;
    const requestedNames = deleteTarget.names;
    const wasSelectionMode = selectionMode;
    setBusy(true);
    try {
      await api.deleteTenants(requestedNames);
      setDeleteTarget(null);
      setSelectionMode(false);
      setSelectedKeys(new Set());
      await loadTenants();
    } catch (cause) {
      const deletionError = messageOf(cause);
      setDeleteTarget(null);
      const refreshed = await loadTenants();
      if (refreshed) {
        const selectedStillExists =
          selectedKey !== null && refreshed.some((row) => tenantKeyOf(row) === selectedKey);
        if (selectedKey !== null && !selectedStillExists) preserveNextComponentError();
        const remaining = requestedNames.filter((name) =>
          refreshed.some((row) => row.kind === "managed" && row.name === name),
        );
        setSelectedKeys(
          wasSelectionMode
            ? new Set(remaining.map((name) => `managed:${name}` as TenantSelectionKey))
            : new Set(),
        );
        setSelectionMode(wasSelectionMode && remaining.length > 0);
      }
      setError(deletionError);
    } finally {
      setBusy(false);
    }
  }

  async function mutateComponent(
    row: ComponentRow,
    install: boolean,
    requestedVersion?: string | null,
  ): Promise<boolean> {
    if (!selected) return false;
    setBusy(true);
    setComponentActionProgress({
      tenantKey: tenantKeyOf(selected),
      kind: row.kind,
      label: componentProgressLabel(row, install),
    });
    try {
      const latest = latestEntryFor(latestSnapshot, row.kind);
      const version = !install
        ? null
        : requestedVersion === undefined
          ? row.supports_version && latest?.state === "available"
            ? latest.version
            : null
          : requestedVersion;
      const result = await api.mutateComponent(
        tenantSelection(selected),
        row.kind,
        install,
        version,
      );
      const operationStarted = result.kind === "operation" && Boolean(onOperation);
      if (result.kind === "operation") onOperation?.(result.operation);
      await loadComponents(selected);
      if (!operationStarted) setComponentActionProgress(null);
      return true;
    } catch (cause) {
      setError(messageOf(cause));
      setComponentActionProgress(null);
      return false;
    } finally {
      setBusy(false);
    }
  }

  function openSpecificVersion(row: ComponentRow, mode: ComponentSpecificVersionTarget["mode"]) {
    if (!selected) return;
    setSpecificVersionTarget({ row, tenantLabel: selected.display_name, mode });
    setSpecificVersion("");
    setSpecificVersionError(null);
  }

  async function submitSpecificVersion() {
    if (!specificVersionTarget || !specificVersionValid) return;
    const installed = await mutateComponent(specificVersionTarget.row, true, specificVersionValue);
    if (installed) {
      setSpecificVersionTarget(null);
    } else {
      setSpecificVersionError(
        `The specific version could not be ${specificVersionTarget.mode === "update" ? "updated" : "installed"}.`,
      );
    }
  }

  return {
    allSelectable,
    attentionComponentCount,
    busy,
    cancelSelection,
    checkingLatest,
    checkForUpdates,
    componentActionProgress,
    componentCatalogLoading,
    componentGroups,
    componentMenuButtons,
    componentMenuItems,
    componentMenuPosition,
    componentMenuRef,
    componentRemoveTarget,
    componentTotalCount,
    copiedHome,
    copyHome,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTenant,
    createTitleId,
    deleteTarget,
    deleteTenants,
    detailHeadingRef,
    detailOpen,
    error,
    expandedComponents,
    hostTenant,
    installedComponentCount,
    latestSnapshot,
    loadComponents,
    loadingTenants,
    managedTenants,
    mutateComponent,
    mutationBusy,
    newName,
    openComponentMenu,
    openSpecificVersion,
    refreshing,
    requestTenantDelete,
    retryTenantPage,
    selected,
    selectedCount,
    selectedHome,
    selectedKey,
    selectedKeys,
    selectableKeys,
    selectionMode,
    setComponentRemoveTarget,
    setComponentMenuPosition,
    setCreateError,
    setCreateOpen,
    setDeleteTarget,
    setExpandedComponents,
    setNewName,
    setOpenComponentMenu,
    setSelectedKey,
    setSelectionMode,
    setSpecificVersion,
    setSpecificVersionError,
    setSpecificVersionTarget,
    setDetailOpen,
    specificVersion,
    specificVersionError,
    specificVersionHelpId,
    specificVersionTarget,
    specificVersionTitleId,
    specificVersionValid,
    specificVersionValidationError,
    submitSpecificVersion,
    tenantCatalogError,
    tenantKindLabel,
    tenantRowButtons,
    toggleAllTenants,
    toggleTenant,
    refreshTenants,
  };
}
