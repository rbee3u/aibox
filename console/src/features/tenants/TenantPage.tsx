import {
  Check,
  ChevronLeft,
  Clipboard,
  Download,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type {
  ComponentKind,
  ComponentLatestSnapshot,
  ComponentRow,
  TenantApi,
} from "@/api/tenants";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { Dialog } from "@/shared/ui/Dialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { TextInput } from "@/shared/ui/FormControls";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { ComponentCatalogSkeleton } from "@/features/tenants/components/ComponentCatalogSkeleton";
import { ComponentRowItem } from "@/features/tenants/components/ComponentRowItem";
import {
  abbreviateTenantHome,
  canonicalComponentStatus,
  COMPONENT_GROUPS,
  compareStableVersions,
  componentLabel,
  componentMenuCoordinates,
  componentRowModel,
  componentProgressLabel,
  hasComponentAttention,
  latestEntryFor,
  relativeTimeLabel,
  tenantSelection,
} from "@/features/tenants/componentCatalog";
import {
  fallbackTenantKey,
  tenantKeyOf,
  tenantLocation,
  type TenantKey,
} from "@/features/tenants/route";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { DNS_LABEL_PATTERN, parseTenantSelectionKey } from "@/api/tenantSelection";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
interface PageProps {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange?: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}
type TenantDeleteTarget = {
  names: string[];
};
type ComponentRemoveTarget = {
  row: ComponentRow;
  tenantLabel: string;
};
type ComponentSpecificVersionTarget = {
  row: ComponentRow;
  tenantLabel: string;
  mode: "install" | "update";
};
type ComponentActionProgress = {
  tenantKey: TenantKey;
  kind: ComponentKind;
  label: string;
};

export function TenantPage({ api, operation, search, onLocationChange, onOperation }: PageProps) {
  const [initialRoute] = useState(() => new URLSearchParams(search));
  const observedSearch = useRef<string | null>(null);
  const normalizedComponentSearch = useRef<string | null>(null);
  const initialKey = parseTenantSelectionKey(initialRoute.get("tenant"));
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(initialKey);
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [componentsTenantKey, setComponentsTenantKey] = useState<string | null>(null);
  const [loadingComponents, setLoadingComponents] = useState(false);
  const [componentActionProgress, setComponentActionProgress] =
    useState<ComponentActionProgress | null>(null);
  const [expandedComponents, setExpandedComponents] = useState<Set<string>>(new Set());
  const [openComponentMenu, setOpenComponentMenu] = useState<ComponentKind | null>(null);
  const [componentMenuPosition, setComponentMenuPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const [latestSnapshot, setLatestSnapshot] = useState<ComponentLatestSnapshot | null>(null);
  const [checkingLatest, setCheckingLatest] = useState(false);
  const [specificVersionTarget, setSpecificVersionTarget] =
    useState<ComponentSpecificVersionTarget | null>(null);
  const [specificVersion, setSpecificVersion] = useState("");
  const [specificVersionError, setSpecificVersionError] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadingTenants, setLoadingTenants] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<TenantKey>>(new Set());
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
  const componentRequest = useRef(0);
  const preserveComponentError = useRef(false);
  const refreshedOperation = useRef<string | null>(null);
  const createTitleId = useId();
  const createHelpId = useId();
  const specificVersionTitleId = useId();
  const specificVersionHelpId = useId();
  const [copiedHome, copyHome] = useClipboardFeedback<string>();
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
      if (onLocationChange) onLocationChange(query, true);
      else
        window.history.replaceState(
          null,
          "",
          `${window.location.pathname}${query.toString() ? `?${query}` : ""}`,
        );
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
  const loadTenants = useCallback(async (): Promise<TenantRow[] | null> => {
    try {
      const rows = await api.listTenants();
      setTenants(rows);
      setSelectedKey((current) => {
        if (current && rows.some((row) => tenantKeyOf(row) === current)) return current;
        const fallback = fallbackTenantKey(rows);
        if (current) {
          setDetailOpen(false);
          onLocationChange?.(new URLSearchParams(), true);
        }
        return fallback;
      });
      setSelectedKeys((current) => {
        const available = new Set(rows.map((row) => tenantKeyOf(row)));
        return new Set(
          [...current].filter(
            (key) => available.has(key) && key !== "host" && key !== "managed:default",
          ),
        );
      });
      setError(null);
      return rows;
    } catch (cause) {
      setError(messageOf(cause));
      return null;
    } finally {
      setLoadingTenants(false);
    }
  }, [api, onLocationChange]);
  useEffect(() => {
    // The page lifecycle synchronizes with the external Tenant catalog.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadTenants();
  }, [loadTenants]);
  const loadComponents = useCallback(
    async (target: TenantRow | null, showLoading = false) => {
      const requestId = ++componentRequest.current;
      if (!target) {
        setComponents([]);
        setComponentsTenantKey(null);
        setLoadingComponents(false);
        return;
      }
      const targetKey = tenantKeyOf(target);
      if (showLoading) {
        setLoadingComponents(true);
        setExpandedComponents(new Set());
        setOpenComponentMenu(null);
      }
      try {
        const rows = await api.listComponents(tenantSelection(target));
        if (requestId !== componentRequest.current) return;
        setComponents(rows);
        setComponentsTenantKey(targetKey);
        setExpandedComponents(new Set());
        setOpenComponentMenu(null);
        if (preserveComponentError.current) preserveComponentError.current = false;
        else setError(null);
      } catch (cause) {
        if (requestId !== componentRequest.current) return;
        if (preserveComponentError.current) preserveComponentError.current = false;
        else setError(messageOf(cause));
      } finally {
        if (requestId === componentRequest.current) setLoadingComponents(false);
      }
    },
    [api],
  );
  useEffect(() => {
    // The selected Tenant determines which external Component catalog is loaded.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadComponents(selected, true);
  }, [loadComponents, selected]);
  useEffect(() => {
    let cancelled = false;
    void api
      .latestComponents()
      .then((snapshot) => {
        if (!cancelled) setLatestSnapshot(snapshot);
      })
      .catch(() => {
        // A missing latest snapshot is an expected first-run state. Local
        // Component inspection remains usable when the read-only endpoint is
        // unavailable.
      });
    return () => {
      cancelled = true;
    };
  }, [api]);
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
    setLoadingTenants(true);
    const rows = await loadTenants();
    if (rows) await loadComponents(selected, true);
  }
  async function checkForUpdates() {
    if (checkingLatest) return;
    setCheckingLatest(true);
    try {
      const [snapshot] = await Promise.all([api.checkLatestComponents(), loadComponents(selected)]);
      setLatestSnapshot(snapshot);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setCheckingLatest(false);
    }
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
      const key = `managed:${created}` as TenantKey;
      setSelectedKey(key);
      setDetailOpen(true);
      onLocationChange?.(tenantLocation(key));
    } catch (cause) {
      setCreateError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }
  function toggleTenant(key: TenantKey) {
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
        if (selectedKey !== null && !selectedStillExists) {
          preserveComponentError.current = true;
        }
        const remaining = requestedNames.filter((name) =>
          refreshed.some((row) => row.kind === "managed" && row.name === name),
        );
        setSelectedKeys(
          wasSelectionMode
            ? new Set(remaining.map((name) => `managed:${name}` as TenantKey))
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
      const operationStarted = "id" in result && Boolean(onOperation);
      if ("id" in result) onOperation?.(result);
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
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError error={error} onRetry={() => void retryTenantPage()} />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${detailOpen ? layout.showsDetail : ""}`}>
        <aside className={styles.tenantCatalog} aria-label="Tenants">
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
                    disabled={selectableKeys.length === 0 || busy}
                    onClick={toggleAllTenants}
                  >
                    {allSelectable ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={layout.selectionDelete}
                    aria-label="Delete selected Tenants"
                    disabled={selectedCount === 0 || mutationBusy}
                    onClick={() =>
                      requestTenantDelete([...selectedKeys].map((key) => key.slice(8)))
                    }
                  >
                    <Trash2 size={14} aria-hidden="true" /> Delete selected
                  </button>
                </div>
              </>
            ) : (
              <div className={layout.toolbarActions}>
                <RefreshButton
                  className={layout.refreshAction}
                  label="Refresh Tenants"
                  busyLabel="Refreshing Tenants"
                  busy={refreshing}
                  disabled={refreshing || loadingTenants}
                  onClick={() => void refreshTenants()}
                >
                  Refresh
                </RefreshButton>
                <button
                  type="button"
                  className={layout.selectionEnter}
                  aria-label="Select Tenants"
                  disabled={selectableKeys.length === 0 || refreshing || loadingTenants || busy}
                  onClick={() => setSelectionMode(true)}
                >
                  <ListChecks size={14} /> Select
                </button>
              </div>
            )}
          </div>
          <div className={layout.list} aria-busy={refreshing || loadingTenants}>
            {loadingTenants ? (
              <Loading />
            ) : (
              <div className={layout.rowGroup}>
                {hostTenant && (
                  <div
                    className={`${layout.row} ${styles.tenantRow} ${selectedKey === "host" ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
                  >
                    <button
                      ref={(element) => {
                        if (element) tenantRowButtons.current.set("host", element);
                        else tenantRowButtons.current.delete("host");
                      }}
                      type="button"
                      className={styles.configRowMain}
                      aria-label={selectionMode ? "Host Tenant cannot be selected" : "Host Tenant"}
                      aria-pressed={!selectionMode && selectedKey === "host"}
                      disabled={refreshing || selectionMode}
                      onClick={() => {
                        setSelectedKey("host");
                        setDetailOpen(true);
                        onLocationChange?.(tenantLocation("host"));
                      }}
                    >
                      <HostTenantIcon size={16} data-icon="host-tenant" />
                      <span className={styles.tenantRowText}>
                        <strong>Host Tenant</strong>
                        <small className={styles.tenantPath} title={hostTenant.home}>
                          {abbreviateTenantHome(hostTenant.home, hostTenant.home)}
                        </small>
                      </span>
                    </button>
                  </div>
                )}
                <div className={layout.divider}>
                  <span>Managed Tenants</span>
                  <IconButton
                    className={layout.addAction}
                    label="Create Managed Tenant"
                    disabled={mutationBusy || refreshing || selectionMode}
                    onClick={() => {
                      setCreateError(null);
                      setCreateOpen(true);
                    }}
                  >
                    <Plus size={15} />
                  </IconButton>
                </div>
                {managedTenants.map((row) => {
                  const key = tenantKeyOf(row);
                  const isDefault = row.name === "default";
                  const selectedForInspection = key === selectedKey;
                  const selectedForDeletion = selectedKeys.has(key);
                  return (
                    <div
                      key={key}
                      className={`${layout.row} ${styles.tenantRow} ${selectedForInspection ? layout.rowInspected : ""} ${selectedForDeletion ? layout.rowSelected : ""} ${selectionMode ? layout.rowSelectable : ""} ${isDefault ? layout.rowProtected : ""}`}
                    >
                      <button
                        ref={(element) => {
                          if (element) tenantRowButtons.current.set(key, element);
                          else tenantRowButtons.current.delete(key);
                        }}
                        type="button"
                        className={styles.configRowMain}
                        aria-label={
                          selectionMode
                            ? isDefault
                              ? "Default Managed Tenant is protected and cannot be selected"
                              : `${selectedForDeletion ? "Deselect" : "Select"} ${row.display_name}`
                            : `${row.display_name}, Managed Tenant`
                        }
                        aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                        disabled={refreshing || (selectionMode && isDefault)}
                        onClick={() => {
                          if (selectionMode) toggleTenant(key);
                          else {
                            setSelectedKey(key);
                            setDetailOpen(true);
                            onLocationChange?.(tenantLocation(key));
                          }
                        }}
                      >
                        <ManagedTenantIcon size={16} data-icon="managed-tenant" />
                        <span className={styles.tenantRowText}>
                          <strong>{row.display_name}</strong>
                          <small className={styles.tenantPath} title={row.home}>
                            {abbreviateTenantHome(row.home, hostTenant?.home ?? null)}
                          </small>
                        </span>
                        {selectionMode && !isDefault && (
                          <span className={layout.selectionIndicator} aria-hidden="true">
                            {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                          </span>
                        )}
                      </button>
                      {!selectionMode && !isDefault && (
                        <div className={layout.rowActions}>
                          <IconButton
                            className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                            label={`Delete Tenant ${row.display_name}`}
                            disabled={mutationBusy}
                            onClick={() => requestTenantDelete([row.name])}
                          >
                            <Trash2 size={15} />
                          </IconButton>
                        </div>
                      )}
                    </div>
                  );
                })}
                {managedTenants.length === 0 && !error && (
                  <EmptyState
                    variant="list"
                    icon={<ManagedTenantIcon size={22} aria-hidden="true" />}
                    title="No Managed Tenants found."
                  />
                )}
              </div>
            )}
          </div>
        </aside>
        <section className={styles.detailPane}>
          {selected ? (
            <>
              <div
                className={`${styles.detailHeader} ${styles.tenantDetailHeader}`}
                data-component-header
              >
                <div className={styles.componentHeaderInner}>
                  <IconButton
                    label="Back to Tenants"
                    onClick={() => {
                      const focusKey = selectedKey;
                      setSelectedKey(null);
                      setDetailOpen(false);
                      onLocationChange?.(new URLSearchParams());
                      window.requestAnimationFrame(() => {
                        if (focusKey) tenantRowButtons.current.get(focusKey)?.focus();
                      });
                    }}
                  >
                    <ChevronLeft size={17} />
                  </IconButton>
                  <div className={styles.componentHeaderIdentity}>
                    <h2 ref={detailHeadingRef} tabIndex={-1}>
                      Components
                    </h2>
                    <div
                      className={styles.componentHeaderContext}
                      aria-label={
                        selected.kind === "host"
                          ? "Selected Tenant: Host Tenant"
                          : `Selected Tenant: ${selected.display_name}, ${tenantKindLabel}`
                      }
                    >
                      <span className={styles.componentTenant}>{selected.display_name}</span>
                      <div className={styles.componentHome}>
                        <span aria-hidden="true">·</span>
                        <code title={selected.home}>{selectedHome}</code>
                        <IconButton
                          className={styles.componentHomeCopy}
                          label={
                            copiedHome === selected.home ? "Tenant Home copied" : "Copy Tenant Home"
                          }
                          onClick={() => void copyHome(selected.home, selected.home)}
                        >
                          {copiedHome === selected.home ? (
                            <Check size={13} />
                          ) : (
                            <Clipboard size={13} />
                          )}
                        </IconButton>
                      </div>
                    </div>
                  </div>
                  <div className={styles.componentHeaderMeta} aria-label="Component summary">
                    {componentCatalogLoading ? (
                      <span className={styles.componentHeaderLoading}>Loading…</span>
                    ) : (
                      <>
                        <span className={styles.componentInstalledSummary}>
                          <strong>{installedComponentCount}</strong>/{componentTotalCount} installed
                        </span>
                        {attentionComponentCount > 0 && (
                          <span className={styles.componentSummaryAttention}>
                            {attentionComponentCount}{" "}
                            {attentionComponentCount === 1 ? "issue" : "issues"}
                          </span>
                        )}
                      </>
                    )}
                    <div className={styles.componentCheckStatus}>
                      {latestSnapshot ? (
                        <time
                          dateTime={latestSnapshot.checked_at}
                          title={new Date(latestSnapshot.checked_at).toLocaleString()}
                        >
                          Checked {relativeTimeLabel(latestSnapshot.checked_at)}
                        </time>
                      ) : (
                        <span>Not checked</span>
                      )}
                    </div>
                    <IconButton
                      className={styles.componentCheckButton}
                      label={checkingLatest ? "Checking for updates" : "Check for updates"}
                      aria-busy={checkingLatest || undefined}
                      disabled={checkingLatest}
                      onClick={() => void checkForUpdates()}
                    >
                      <RefreshCw
                        className={checkingLatest ? "spin" : undefined}
                        size={15}
                        aria-hidden="true"
                      />
                    </IconButton>
                  </div>
                </div>
              </div>
              <div
                className={styles.componentViewport}
                aria-busy={componentCatalogLoading || undefined}
              >
                <div className={styles.componentCatalogContent}>
                  {componentCatalogLoading ? (
                    <ComponentCatalogSkeleton host={selected.kind === "host"} />
                  ) : (
                    <div className={styles.componentCatalog} aria-label="Components">
                      {componentGroups.map((group) => (
                        <section
                          className={styles.componentGroup}
                          aria-labelledby={`component-group-${group.id}`}
                          key={group.id}
                        >
                          <div className={styles.componentGroupHeader}>
                            <h3 id={`component-group-${group.id}`}>{group.label}</h3>
                          </div>
                          <div role="list" aria-label={`${group.label} Components`}>
                            {group.rows.map((row) => {
                              const model = componentRowModel(row, latestSnapshot);
                              const rowProgress =
                                componentActionProgress?.tenantKey === selectedKey &&
                                componentActionProgress.kind === row.kind
                                  ? componentActionProgress.label
                                  : null;
                              return (
                                <ComponentRowItem
                                  key={row.kind}
                                  row={row}
                                  model={model}
                                  expanded={expandedComponents.has(row.kind)}
                                  progressLabel={rowProgress}
                                  busy={busy}
                                  mutationBusy={mutationBusy}
                                  openMenu={openComponentMenu}
                                  menuPosition={componentMenuPosition}
                                  menuRef={componentMenuRef}
                                  onToggleExpanded={() =>
                                    setExpandedComponents((current) => {
                                      const next = new Set(current);
                                      if (next.has(row.kind)) next.delete(row.kind);
                                      else next.add(row.kind);
                                      return next;
                                    })
                                  }
                                  onRetryInspection={() => void loadComponents(selected)}
                                  onInstall={() => void mutateComponent(row, true)}
                                  onRemove={() =>
                                    setComponentRemoveTarget({
                                      row,
                                      tenantLabel: selected.display_name,
                                    })
                                  }
                                  onOpenSpecificVersion={() =>
                                    openSpecificVersion(row, model.specificVersionMode)
                                  }
                                  onMenuPosition={setComponentMenuPosition}
                                  onOpenMenu={setOpenComponentMenu}
                                  registerMenuButton={(element) => {
                                    if (element)
                                      componentMenuButtons.current.set(row.kind, element);
                                    else componentMenuButtons.current.delete(row.kind);
                                  }}
                                  registerMenuItem={(element) => {
                                    if (element) componentMenuItems.current.set(row.kind, element);
                                    else componentMenuItems.current.delete(row.kind);
                                  }}
                                />
                              );
                            })}
                          </div>
                        </section>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            </>
          ) : (
            <EmptyState
              variant="detail"
              icon={<ManagedTenantIcon size={26} aria-hidden="true" />}
              title="Select a Tenant"
              description="Choose a Tenant to inspect its Components."
            />
          )}
        </section>
      </div>
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
              if (createNameValid && !mutationBusy) void createTenant();
            }}
          >
            <h2 id={createTitleId}>Create Managed Tenant</h2>
            <label>
              Name
              <TextInput
                autoFocus
                aria-label="Tenant name"
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
              <div className={layout.alertBanner} role="alert">
                Enter a valid lowercase DNS label.
              </div>
            )}
            {createError && <div className={layout.alertBanner}>{createError}</div>}
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
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.names[0]}?`}
          description={
            <p className={layout.dialogDescription}>
              This permanently deletes the Tenant Home, Sessions, Components state, and Named
              Configs for this Tenant.
            </p>
          }
          confirmation={deleteTarget.names[0]}
          confirmLabel="Delete Tenant"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Managed Tenants?"
          description={
            <>
              <p className={layout.dialogDescription}>
                This permanently deletes each Tenant Home, its Sessions and Components state, and
                its Named Configs.
              </p>
              <div className={layout.planList}>
                {deleteTarget.names.map((name) => (
                  <code key={name}>{name}</code>
                ))}
              </div>
            </>
          }
          confirmLabel="Delete selected"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {specificVersionTarget && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={specificVersionTitleId}
          busy={mutationBusy}
          onCancel={() => setSpecificVersionTarget(null)}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (specificVersionValid && !mutationBusy) void submitSpecificVersion();
            }}
          >
            <h2 id={specificVersionTitleId}>
              {specificVersionTarget.mode === "update"
                ? `Update ${componentLabel(specificVersionTarget.row.kind)} version`
                : `Install ${componentLabel(specificVersionTarget.row.kind)} version`}
            </h2>
            <p className={layout.dialogDescription}>
              Tenant: <strong>{specificVersionTarget.tenantLabel}</strong>
            </p>
            <label>
              Version
              <TextInput
                autoFocus
                aria-label="Component version"
                value={specificVersion}
                placeholder="X.Y.Z"
                onChange={(event) => {
                  setSpecificVersion(event.target.value);
                  setSpecificVersionError(null);
                }}
                aria-invalid={Boolean(specificVersionValidationError)}
                aria-describedby={specificVersionHelpId}
              />
            </label>
            <p id={specificVersionHelpId} className={layout.dialogDescription}>
              {specificVersionTarget.mode === "update"
                ? `Enter a stable version newer than v${specificVersionTarget.row.version}.`
                : "Enter a stable version in X.Y.Z form."}
            </p>
            {specificVersionValidationError && (
              <div className={layout.alertBanner} role="alert">
                {specificVersionValidationError}
              </div>
            )}
            {specificVersionError && (
              <div className={layout.alertBanner} role="alert">
                {specificVersionError}
              </div>
            )}
            <div className={styles.dialogActions}>
              <button
                type="button"
                onClick={() => setSpecificVersionTarget(null)}
                disabled={mutationBusy}
              >
                Cancel
              </button>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!specificVersionValid || mutationBusy}
              >
                {mutationBusy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Download size={14} />
                )}
                {mutationBusy
                  ? specificVersionTarget.mode === "update"
                    ? "Updating…"
                    : "Installing…"
                  : specificVersionTarget.mode === "update"
                    ? "Update version"
                    : "Install version"}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {componentRemoveTarget && (
        <ConfirmDialog
          title={`Remove ${componentLabel(componentRemoveTarget.row.kind)}?`}
          description={
            <div className={layout.dialogDescription}>
              <p>
                Tenant: <strong>{componentRemoveTarget.tenantLabel}</strong>
              </p>
              <p>
                Current state:{" "}
                <strong>{canonicalComponentStatus(componentRemoveTarget.row)}</strong>
              </p>
              <p>
                Existing Component-owned state will be deleted. Workspace environments and
                user-owned package, cache, credential, and configuration state are preserved.
              </p>
            </div>
          }
          confirmLabel="Remove Component"
          busy={mutationBusy}
          onCancel={() => setComponentRemoveTarget(null)}
          onConfirm={() => {
            const row = componentRemoveTarget.row;
            void mutateComponent(row, false).then(() => setComponentRemoveTarget(null));
          }}
        />
      )}
    </div>
  );
}
