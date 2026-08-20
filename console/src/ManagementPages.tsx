/* eslint-disable react-hooks/set-state-in-effect */

import {
  AlertTriangle,
  Box,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronUp,
  CircleStop,
  Download,
  Eye,
  EyeOff,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  X,
} from "lucide-react";
import { basicSetup } from "codemirror";
import { json } from "@codemirror/lang-json";
import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { EditorView, keymap } from "@codemirror/view";
import { EditorState, type Extension } from "@codemirror/state";
import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { searchKeymap } from "@codemirror/search";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { tags } from "@lezer/highlight";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { ControlApi, decodeBase64, encodeBase64, scopeBody, scopeQuery } from "./controlApi";
import type {
  Agent,
  ApplicationStatus,
  ComponentRow,
  ConfigCatalogEntry,
  ConfigFileData,
  ConfigVisualField,
  ConfigListData,
  Operation,
  Prompt,
  PropagationPreview,
  PropagationReport,
  PropagationOutcome,
  Scope,
  SessionListData,
  SessionRow,
  TenantRow,
} from "./controlApi";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { Dialog } from "./components/Dialog";
import { EmptyState } from "./components/EmptyState";
import { IconButton } from "./components/IconButton";
import { IssueIndicator, type IssueTone } from "./components/IssueIndicator";
import { NotificationCenter } from "./components/NotificationCenter";
import { useFailureNotifications } from "./useFailureNotifications";
import { AgentIcon } from "./icons";
import { formatTimestamp } from "./utils";
import { resourceIcons, type ModuleId } from "./consoleIcons";
import styles from "./ManagementPages.module.css";

const ComponentIcon = resourceIcons.component;
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
const SessionIcon = resourceIcons.session;

const configHighlightStyle = HighlightStyle.define([
  { tag: tags.propertyName, class: "cm-config-key" },
  { tag: tags.string, class: "cm-config-string" },
  { tag: tags.number, class: "cm-config-number" },
  { tag: [tags.bool, tags.null, tags.atom], class: "cm-config-boolean" },
  { tag: tags.comment, class: "cm-config-comment" },
  { tag: tags.invalid, class: "cm-config-invalid" },
]);

function codeMirrorCspNonce(): string {
  return document.querySelector<HTMLMetaElement>('meta[name="aibox-csp-nonce"]')?.content ?? "";
}

interface PageProps {
  api: ControlApi;
  operation?: Operation | null;
  locationVersion?: number;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange?: (module: ModuleId, query: URLSearchParams, replace?: boolean) => void;
  onOperation?: (operation: Operation) => void;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function PageError({ error, onRetry }: { error: string | null; onRetry?: () => void }) {
  if (!error) return null;
  return (
    <div className={styles.errorBanner} role="alert">
      <AlertTriangle size={16} aria-hidden="true" />
      <span>{error}</span>
      {onRetry && (
        <button type="button" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

function Loading() {
  return (
    <div className={styles.loading}>
      <LoaderCircle className="spin" size={22} aria-label="Loading" />
    </div>
  );
}

function MutationUnavailable({ operation }: { operation?: Operation | null }) {
  if (operation?.state !== "running") return null;
  return (
    <div className={styles.mutationUnavailable} role="status">
      A Management Operation is active. Changes are temporarily unavailable; browsing and refresh
      remain available.
    </div>
  );
}

function tenantScope(row: TenantRow): Scope {
  return row.kind === "host" ? { scope: "host" } : { scope: "managed", tenant: row.name! };
}

type TenantKey = "host" | `managed:${string}`;
type TenantDeleteTarget = { names: string[] };
type ComponentRemoveTarget = { row: ComponentRow; tenantLabel: string };

const COMPONENT_LABELS: Record<string, string> = {
  "claude-statusline": "Claude status line",
  "codex-statusline": "Codex status line",
  rust: "Rust toolchain",
  go: "Go toolchain",
};

function componentLabel(kind: string): string {
  return COMPONENT_LABELS[kind] ?? kind;
}

function componentPresentation(row: ComponentRow): {
  statusLabel: string;
  statusClass: string;
  primaryAction: "Install" | "Repair" | "Restore" | "Retry inspection" | null;
  canRemove: boolean;
  detail: string;
} {
  if (row.error || !row.status) {
    return {
      statusLabel: "Inspection error",
      statusClass: styles.errorStatus,
      primaryAction: "Retry inspection",
      canRemove: false,
      detail: row.error ?? "Component state could not be inspected safely.",
    };
  }
  switch (row.status) {
    case "not-installed":
      return {
        statusLabel: "Not installed",
        statusClass: styles.neutralStatus,
        primaryAction: "Install",
        canRemove: false,
        detail: "No Component-owned state was detected.",
      };
    case "installed":
      return {
        statusLabel: "Installed",
        statusClass: styles.goodStatus,
        primaryAction: null,
        canRemove: true,
        detail: row.version ? `Installed version ${row.version}.` : "Installed and healthy.",
      };
    case "incomplete":
      return {
        statusLabel: "Incomplete",
        statusClass: styles.warnStatus,
        primaryAction: "Repair",
        canRemove: true,
        detail: "Recognizable Component state is incomplete and can be repaired.",
      };
    case "modified":
      return {
        statusLabel: "Modified",
        statusClass: styles.warnStatus,
        primaryAction: "Restore",
        canRemove: true,
        detail: "Detected state differs from the AIBox definition.",
      };
    case "unmanaged":
      return {
        statusLabel: "Unmanaged",
        statusClass: styles.warnStatus,
        primaryAction: null,
        canRemove: true,
        detail: "Detected state is not owned by AIBox and will not be overwritten.",
      };
    default:
      return {
        statusLabel: row.status,
        statusClass: styles.warnStatus,
        primaryAction: null,
        canRemove: false,
        detail: "This Component state is not recognized by this Console version.",
      };
  }
}

function tenantKeyOf(row: TenantRow): TenantKey {
  return row.kind === "host" ? "host" : `managed:${row.name}`;
}

function tenantKeyFromParam(value: string | null): TenantKey | null {
  if (value === "host") return "host";
  if (value?.startsWith("managed:") && CONFIG_NAME_PATTERN.test(value.slice(8))) {
    return value as TenantKey;
  }
  return null;
}

function pageSearch(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}

function changePageLocation(
  module: ModuleId,
  query: URLSearchParams,
  onLocationChange?: PageProps["onLocationChange"],
  replace = false,
) {
  onLocationChange?.(module, query, replace);
}

function tenantLocation(key: TenantKey | null, component?: string | null): URLSearchParams {
  const query = new URLSearchParams();
  if (key) query.set("scope", key);
  if (key && component) query.set("component", component);
  return query;
}

function abbreviateTenantHome(path: string, hostHome: string | null): string {
  if (!hostHome) return path;
  if (path === hostHome) return "~";
  const prefix = hostHome.endsWith("/") ? hostHome : `${hostHome}/`;
  return path.startsWith(prefix) ? `~/${path.slice(prefix.length)}` : path;
}

export function TenantPage({
  api,
  operation,
  locationVersion = 0,
  onLocationChange,
  onOperation,
}: PageProps) {
  const [initialRoute] = useState(pageSearch);
  const observedLocationVersion = useRef(locationVersion);
  const initialKey = tenantKeyFromParam(initialRoute.get("scope"));
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(initialKey);
  const [selectedComponent, setSelectedComponent] = useState<string | null>(
    initialKey ? initialRoute.get("component") : null,
  );
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [versions, setVersions] = useState<Record<string, string>>({});
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
  const preserveComponentError = useRef(false);
  const refreshedOperation = useRef<string | null>(null);
  const createTitleId = useId();
  const createHelpId = useId();
  const selected = tenants.find((row) => tenantKeyOf(row) === selectedKey) ?? null;
  const hostTenant = tenants.find((row) => row.kind === "host") ?? null;
  const managedTenants = useMemo(
    () =>
      tenants
        .filter(
          (row): row is TenantRow & { kind: "managed"; name: string } =>
            row.kind === "managed" && Boolean(row.name),
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
  const createNameValid = CONFIG_NAME_PATTERN.test(newName);
  const mutationBusy = busy || operation?.state === "running";

  useEffect(() => {
    if (observedLocationVersion.current === locationVersion) return;
    observedLocationVersion.current = locationVersion;
    const query = pageSearch();
    const key = tenantKeyFromParam(query.get("scope"));
    setSelectedKey(key);
    setSelectedComponent(key ? query.get("component") : null);
    setDetailOpen(key !== null);
  }, [locationVersion]);

  useEffect(() => {
    if (!detailOpen || !selectedKey || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() => detailHeadingRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, selectedKey]);

  const loadTenants = useCallback(async (): Promise<TenantRow[] | null> => {
    try {
      const rows = await api.get<TenantRow[]>("/_aibox/api/tenants");
      setTenants(rows);
      setSelectedKey((current) => {
        if (current && rows.some((row) => tenantKeyOf(row) === current)) return current;
        const fallback =
          rows.find((row) => row.kind === "managed" && row.name === "default") ??
          rows.find((row) => row.kind === "managed") ??
          rows.find((row) => row.kind === "host");
        if (current) {
          setSelectedComponent(null);
          setDetailOpen(false);
          changePageLocation("tenants", new URLSearchParams(), onLocationChange, true);
        }
        return fallback ? tenantKeyOf(fallback) : null;
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
  useEffect(() => void loadTenants(), [loadTenants]);

  const loadComponents = useCallback(async () => {
    if (!selected) {
      setComponents([]);
      return;
    }
    try {
      const query = scopeQuery(tenantScope(selected));
      const rows = await api.get<ComponentRow[]>(`/_aibox/api/components?${query}`);
      setComponents(rows);
      if (selectedComponent && !rows.some((row) => row.kind === selectedComponent)) {
        setSelectedComponent(null);
        changePageLocation(
          "tenants",
          tenantLocation(tenantKeyOf(selected)),
          onLocationChange,
          true,
        );
      }
      if (preserveComponentError.current) preserveComponentError.current = false;
      else setError(null);
    } catch (cause) {
      if (preserveComponentError.current) preserveComponentError.current = false;
      else setError(messageOf(cause));
    }
  }, [api, onLocationChange, selected, selectedComponent]);
  useEffect(() => void loadComponents(), [loadComponents]);

  useEffect(() => {
    if (!operation || operation.state === "running" || refreshedOperation.current === operation.id)
      return;
    refreshedOperation.current = operation.id;
    void loadTenants();
    void loadComponents();
  }, [loadComponents, loadTenants, operation]);

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
    if (rows) await loadComponents();
  }

  async function createTenant() {
    if (!createNameValid) return;
    setBusy(true);
    try {
      await api.post("/_aibox/api/tenants", { name: newName });
      const created = newName;
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadTenants();
      const key = `managed:${created}` as TenantKey;
      setSelectedKey(key);
      setSelectedComponent(null);
      setDetailOpen(true);
      changePageLocation("tenants", tenantLocation(key), onLocationChange);
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
      await api.post("/_aibox/api/tenants/delete", {
        names: requestedNames,
        all: false,
        confirmation: requestedNames.length === 1 ? requestedNames[0] : "",
      });
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

  async function mutateComponent(row: ComponentRow, install: boolean) {
    if (!selected) return;
    setBusy(true);
    try {
      const path = install ? "install" : "remove";
      const result = await api.post<Operation | object>(`/_aibox/api/components/${path}`, {
        ...scopeBody(tenantScope(selected)),
        component: row.kind,
        version: versions[row.kind] || null,
      });
      if ("id" in result) onOperation?.(result);
      await loadComponents();
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={`${styles.page} ${styles.catalogPage}`}>
      <PageError error={error} onRetry={() => void retryTenantPage()} />
      <MutationUnavailable operation={operation} />
      <div
        className={`${styles.splitLayout} ${styles.tenantLayout} ${detailOpen ? styles.hasSelection : ""}`}
      >
        <aside className={styles.tenantCatalog} aria-label="Tenants">
          <div className={styles.sessionToolbar}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={styles.sessionCancelSelection}
                  disabled={busy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={styles.sessionSelectionActions}>
                  <span className={styles.sessionSelectionCount}>{selectedCount} selected</span>
                  <button
                    type="button"
                    className={styles.sessionSelectAll}
                    disabled={selectableKeys.length === 0 || busy}
                    onClick={toggleAllTenants}
                  >
                    {allSelectable ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={styles.sessionDeleteSelected}
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
              <div className={styles.sessionHeaderActions}>
                <IconButton
                  className={styles.sessionRefresh}
                  label={refreshing ? "Refreshing Tenants" : "Refresh Tenants"}
                  aria-busy={refreshing}
                  disabled={refreshing || loadingTenants}
                  onClick={() => void refreshTenants()}
                >
                  <RefreshCw className={refreshing ? "spin" : undefined} size={14} />
                </IconButton>
                <button
                  type="button"
                  className={styles.sessionSelect}
                  aria-label="Select Tenants"
                  disabled={selectableKeys.length === 0 || refreshing || loadingTenants || busy}
                  onClick={() => setSelectionMode(true)}
                >
                  <ListChecks size={14} /> Select
                </button>
              </div>
            )}
          </div>
          <div className={styles.configList} aria-busy={refreshing || loadingTenants}>
            {loadingTenants ? (
              <Loading />
            ) : (
              <div className={styles.configRowGroup}>
                {hostTenant && (
                  <div
                    className={`${styles.configRow} ${styles.tenantRow} ${selectedKey === "host" ? styles.configRowInspected : ""} ${selectionMode ? `${styles.configRowSelection} ${styles.configRowProtected}` : ""}`}
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
                        setSelectedComponent(null);
                        setDetailOpen(true);
                        changePageLocation("tenants", tenantLocation("host"), onLocationChange);
                      }}
                    >
                      <HostTenantIcon size={16} data-icon="host-tenant" />
                      <span className={styles.tenantRowText}>
                        <strong>Host Tenant</strong>
                        <small className={styles.tenantPath} title={hostTenant.home}>
                          Console-only · {abbreviateTenantHome(hostTenant.home, hostTenant.home)}
                        </small>
                      </span>
                      <span className={styles.hostRiskBadge}>Host risk</span>
                      {selectionMode && <span className={styles.configProtected}>Protected</span>}
                    </button>
                  </div>
                )}
                <div className={styles.catalogDivider}>
                  <span>Managed Tenants</span>
                  <IconButton
                    className={styles.configAddButton}
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
                      className={`${styles.configRow} ${styles.tenantRow} ${selectedForInspection ? styles.configRowInspected : ""} ${selectedForDeletion ? styles.configRowSelected : ""} ${selectionMode ? styles.configRowSelection : ""} ${isDefault ? styles.configRowProtected : ""}`}
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
                            : `${row.display_name}, Managed Tenant${isDefault ? ", Default, Protected" : ""}`
                        }
                        aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                        disabled={refreshing || (selectionMode && isDefault)}
                        onClick={() => {
                          if (selectionMode) toggleTenant(key);
                          else {
                            setSelectedKey(key);
                            setSelectedComponent(null);
                            setDetailOpen(true);
                            changePageLocation("tenants", tenantLocation(key), onLocationChange);
                          }
                        }}
                      >
                        <ManagedTenantIcon size={16} data-icon="managed-tenant" />
                        <span className={styles.tenantRowText}>
                          <strong>{row.display_name}</strong>
                          <small className={styles.tenantPath} title={row.home}>
                            Managed Tenant ·{" "}
                            {abbreviateTenantHome(row.home, hostTenant?.home ?? null)}
                          </small>
                        </span>
                        {isDefault && <span className={styles.defaultBadge}>Default</span>}
                        {isDefault && <span className={styles.configProtected}>Protected</span>}
                        {selectionMode && !isDefault && (
                          <span className={styles.sessionSelectionIndicator} aria-hidden="true">
                            {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                          </span>
                        )}
                      </button>
                      {!selectionMode && !isDefault && (
                        <div className={styles.configRowActions}>
                          <IconButton
                            className={`${styles.configRowAction} ${styles.configDeleteAction}`}
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
              <div className={styles.detailHeader}>
                <IconButton
                  label="Back to Tenants"
                  onClick={() => {
                    const focusKey = selectedKey;
                    setSelectedKey(null);
                    setSelectedComponent(null);
                    setDetailOpen(false);
                    changePageLocation("tenants", new URLSearchParams(), onLocationChange);
                    window.requestAnimationFrame(() => {
                      if (focusKey) tenantRowButtons.current.get(focusKey)?.focus();
                    });
                  }}
                >
                  <ChevronLeft size={17} />
                </IconButton>
                <div>
                  <h2 ref={detailHeadingRef} tabIndex={-1}>
                    {selected.display_name}
                  </h2>
                  <code>{selected.home}</code>
                  {selected.kind === "host" && (
                    <span className={styles.hostRiskNotice}>
                      Console-only · Changes affect native files in the Host Home.
                    </span>
                  )}
                </div>
              </div>
              <div className={styles.sectionHeading}>
                <div>
                  <h3>Components</h3>
                  <span>{components.length} available</span>
                </div>
                <IconButton label="Refresh Components" onClick={() => void loadComponents()}>
                  <RefreshCw size={16} />
                </IconButton>
              </div>
              <div className={styles.tableList}>
                {components.map((row) => {
                  const presentation = componentPresentation(row);
                  const label = componentLabel(row.kind);
                  const supportsVersion =
                    row.supports_version &&
                    (presentation.primaryAction === "Install" ||
                      presentation.primaryAction === "Repair" ||
                      presentation.primaryAction === "Restore");
                  return (
                    <div
                      className={`${styles.componentRow} ${selectedComponent === row.kind ? styles.componentRowSelected : ""}`}
                      key={row.kind}
                    >
                      <button
                        type="button"
                        className={styles.componentRowSelect}
                        aria-pressed={selectedComponent === row.kind}
                        onClick={() => {
                          if (!selected) return;
                          const key = tenantKeyOf(selected);
                          setSelectedComponent(row.kind);
                          changePageLocation(
                            "tenants",
                            tenantLocation(key, row.kind),
                            onLocationChange,
                          );
                        }}
                      >
                        <ComponentIcon size={17} aria-hidden="true" />
                        <span>
                          <strong>{label}</strong>
                          <small>{presentation.detail}</small>
                        </span>
                      </button>
                      {supportsVersion && (
                        <input
                          aria-label={`${label} version`}
                          placeholder="stable"
                          value={versions[row.kind] ?? ""}
                          onChange={(event) =>
                            setVersions((value) => ({ ...value, [row.kind]: event.target.value }))
                          }
                        />
                      )}
                      <span className={presentation.statusClass}>{presentation.statusLabel}</span>
                      <div className={styles.componentActions}>
                        {presentation.primaryAction === "Retry inspection" ? (
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() => void loadComponents()}
                          >
                            <RefreshCw size={14} /> Retry inspection
                          </button>
                        ) : presentation.primaryAction ? (
                          <button
                            type="button"
                            disabled={mutationBusy}
                            onClick={() => void mutateComponent(row, true)}
                          >
                            <Download size={14} /> {presentation.primaryAction}
                          </button>
                        ) : null}
                        {presentation.canRemove && (
                          <button
                            type="button"
                            className={styles.componentRemoveAction}
                            disabled={mutationBusy}
                            onClick={() =>
                              setComponentRemoveTarget({
                                row,
                                tenantLabel: selected.display_name,
                              })
                            }
                          >
                            <Trash2 size={14} />
                            {row.status === "unmanaged" ? "Remove detected state" : "Remove"}
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
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
          className={styles.dialog}
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
              <input
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
            <p id={createHelpId} className={styles.dialogDescription}>
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
              <button
                className={styles.primaryButton}
                type="submit"
                disabled={!createNameValid || mutationBusy}
              >
                {busy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Plus size={14} />
                )}
                {busy ? "Creating…" : "Create"}
              </button>
            </div>
          </form>
        </Dialog>
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.names[0]}?`}
          description={
            <p className={styles.dialogDescription}>
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
              <p className={styles.dialogDescription}>
                This permanently deletes each Tenant Home, its Sessions and Components state, and
                its Named Configs.
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
          onConfirm={() => void deleteTenants()}
        />
      )}
      {componentRemoveTarget && (
        <ConfirmDialog
          title={`Remove ${componentLabel(componentRemoveTarget.row.kind)}?`}
          description={
            <div className={styles.dialogDescription}>
              <p>
                Tenant: <strong>{componentRemoveTarget.tenantLabel}</strong>
              </p>
              <p>
                Current state:{" "}
                <strong>{componentPresentation(componentRemoveTarget.row).statusLabel}</strong>
              </p>
              <p>
                Existing Component-owned state will be deleted. Cargo and GOPATH user state is
                preserved for toolchains.
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

function useTenants(api: ControlApi) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    let disposed = false;
    setLoading(true);
    void api
      .get<TenantRow[]>("/_aibox/api/tenants")
      .then((rows) => {
        if (disposed) return;
        setTenants(rows);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!disposed) setError(messageOf(cause));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [api, generation]);
  return {
    tenants,
    loading,
    error,
    retry: () => setGeneration((value) => value + 1),
  };
}

type ConfigSelection = { current: true; config?: never } | { current: false; config: string };
type ConfigScopeKey = "host" | `managed:${string}`;
type ConfigDeleteTarget = { names: string[] };
type ConfigApplyTarget = { name: string };
type ConfigPendingAction = { run: () => void | Promise<void> };

const CONFIG_NAME_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

function configScopeKey(scope: Scope): ConfigScopeKey {
  return scope.scope === "host" ? "host" : `managed:${scope.tenant}`;
}

function scopeFromConfigKey(key: ConfigScopeKey): Scope {
  return key === "host" ? { scope: "host" } : { scope: "managed", tenant: key.slice(8) };
}

interface ConfigRouteState {
  scope: Scope;
  agent: Agent;
  selection: ConfigSelection;
  file: string | null;
  detailOpen: boolean;
}

function readConfigRoute(): ConfigRouteState {
  const query = pageSearch();
  const scopeKey = tenantKeyFromParam(query.get("scope")) ?? "managed:default";
  const agent = query.get("agent") === "claude" ? "claude" : "codex";
  const config = query.get("config");
  const current = query.get("current") === "1";
  const detailOpen = current || (config !== null && CONFIG_NAME_PATTERN.test(config));
  return {
    scope: scopeFromConfigKey(scopeKey),
    agent,
    selection:
      !current && config && CONFIG_NAME_PATTERN.test(config)
        ? { current: false, config }
        : { current: true },
    file: detailOpen ? query.get("file") : null,
    detailOpen,
  };
}

function configLocation(
  scope: Scope,
  agent: Agent,
  selection: ConfigSelection | null,
  file?: string | null,
): URLSearchParams {
  const query = new URLSearchParams();
  query.set("scope", configScopeKey(scope));
  query.set("agent", agent);
  if (selection?.current) query.set("current", "1");
  else if (selection) query.set("config", selection.config);
  if (selection && file) query.set("file", file);
  return query;
}

interface ConfigIssuePresentation {
  tone: IssueTone;
  label: string;
  message: string;
  accessibleLabel: string;
}

type ConfigVisualFieldInput = Pick<ConfigVisualField, "path" | "included"> & {
  value?: string | boolean;
};

function configIssuePresentation(entry: ConfigCatalogEntry): ConfigIssuePresentation | null {
  if (entry.state === "ready") return null;
  const incomplete = entry.state === "incomplete";
  const tone = incomplete ? "warning" : "error";
  const label = incomplete ? "Incomplete Config" : "Invalid Config";
  const message =
    entry.detail ??
    (incomplete
      ? "Required Config files are missing. Use Repair to restore this Named Config."
      : "This Named Config cannot be safely used.");
  const toneLabel = incomplete ? "warning" : "error";
  return {
    tone,
    label,
    message,
    accessibleLabel: `Config ${toneLabel}: ${label}. ${message}`,
  };
}

function configIssueDescriptionId(scope: Scope, agent: Agent, name: string): string {
  return `config-issue-${configScopeKey(scope).replace(":", "-")}-${agent}-${name}`;
}

function propagationGroup(
  status: PropagationOutcome["status"],
): "updated" | "skipped" | "attention" {
  if (status === "updated") return "updated";
  if (status === "unchanged") return "skipped";
  return "attention";
}

function propagationDetail(outcome: PropagationOutcome): string | null {
  const timestamps = [
    outcome.source_last_refresh ? `source ${outcome.source_last_refresh}` : null,
    outcome.target_last_refresh ? `target ${outcome.target_last_refresh}` : null,
    outcome.last_refresh ? `last refresh ${outcome.last_refresh}` : null,
  ].filter(Boolean);
  return [outcome.reason, ...timestamps].filter(Boolean).join(" · ") || null;
}

function VisualConfigFields({
  fields,
  onChange,
}: {
  fields: ConfigVisualField[];
  onChange: (path: string, update: Partial<ConfigVisualField>) => void;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const groups = useMemo(() => {
    const grouped = new Map<string, ConfigVisualField[]>();
    for (const field of fields)
      grouped.set(field.group, [...(grouped.get(field.group) ?? []), field]);
    return [...grouped.entries()];
  }, [fields]);
  return (
    <div className={styles.visualEditor}>
      {groups.map(([group, groupFields]) => (
        <section className={styles.visualGroup} key={group}>
          <header>
            <h3>{group}</h3>
            <span>Include fields only when this Named Config should project them.</span>
          </header>
          <div className={styles.visualFieldGrid}>
            {groupFields.map((field) => {
              const value = field.value ?? (field.value_kind === "bool" ? false : "");
              const fieldId = `config-field-${field.path.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
              const descriptionId = `${fieldId}-description`;
              const isRevealed = revealed.has(field.path);
              const hasSuggestions = field.suggestions.length > 0;
              const isCustom =
                hasSuggestions && typeof value === "string" && !field.suggestions.includes(value);
              return (
                <article
                  className={styles.visualField}
                  key={field.path}
                  role="group"
                  aria-labelledby={fieldId}
                  aria-describedby={descriptionId}
                >
                  <div className={styles.visualFieldHeader}>
                    <label>
                      <input
                        type="checkbox"
                        aria-label={`Include ${field.label}`}
                        checked={field.included}
                        onChange={(event) =>
                          onChange(field.path, { included: event.target.checked })
                        }
                      />
                      <span>
                        <strong id={fieldId}>{field.label}</strong>
                        <code>{field.path}</code>
                      </span>
                    </label>
                  </div>
                  <p id={descriptionId}>{field.description}</p>
                  {field.value_kind === "bool" ? (
                    <label className={styles.visualValueControl}>
                      <input
                        type="checkbox"
                        aria-label={`${field.label} value`}
                        aria-describedby={descriptionId}
                        checked={Boolean(value)}
                        disabled={!field.included}
                        onChange={(event) => onChange(field.path, { value: event.target.checked })}
                      />
                      <span>{value ? "Enabled" : "Disabled"}</span>
                    </label>
                  ) : hasSuggestions ? (
                    <>
                      <select
                        aria-label={`${field.label} value`}
                        aria-describedby={descriptionId}
                        disabled={!field.included}
                        value={isCustom ? "__custom" : String(value)}
                        onChange={(event) => {
                          if (event.target.value === "__custom")
                            onChange(field.path, { value: "" });
                          else onChange(field.path, { value: event.target.value });
                        }}
                      >
                        <option value="">Select a value</option>
                        {field.suggestions.map((suggestion) => (
                          <option key={suggestion} value={suggestion}>
                            {suggestion}
                          </option>
                        ))}
                        <option value="__custom">Custom</option>
                      </select>
                      {(isCustom || value === "") && (
                        <input
                          className={styles.visualCustomInput}
                          disabled={!field.included}
                          value={String(value)}
                          onChange={(event) => onChange(field.path, { value: event.target.value })}
                          aria-label={`${field.label} custom value`}
                          aria-describedby={descriptionId}
                        />
                      )}
                    </>
                  ) : (
                    <div className={styles.visualTextControl}>
                      <input
                        type={field.sensitive && !isRevealed ? "password" : "text"}
                        disabled={!field.included}
                        value={String(value)}
                        onChange={(event) => onChange(field.path, { value: event.target.value })}
                        aria-label={field.label}
                        aria-describedby={descriptionId}
                      />
                      {field.sensitive && (
                        <IconButton
                          label={isRevealed ? `Hide ${field.label}` : `Show ${field.label}`}
                          onClick={() =>
                            setRevealed((current) => {
                              const next = new Set(current);
                              if (next.has(field.path)) next.delete(field.path);
                              else next.add(field.path);
                              return next;
                            })
                          }
                        >
                          {isRevealed ? <EyeOff size={14} /> : <Eye size={14} />}
                        </IconButton>
                      )}
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}

export function ConfigPage({
  api,
  operation,
  locationVersion = 0,
  onDirtyChange,
  onLocationChange,
}: PageProps) {
  const [initialRoute] = useState(readConfigRoute);
  const observedLocationVersion = useRef(locationVersion);
  const {
    tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useTenants(api);
  const [scope, setScope] = useState<Scope>(initialRoute.scope);
  const [agent, setAgent] = useState<Agent>(initialRoute.agent);
  const [catalog, setCatalog] = useState<ConfigListData | null>(null);
  const [selection, setSelection] = useState<ConfigSelection>(initialRoute.selection);
  const [file, setFile] = useState<string | null>(initialRoute.file);
  const [snapshot, setSnapshot] = useState<ConfigFileData | null>(null);
  const [editor, setEditor] = useState("");
  const [editorMode, setEditorMode] = useState<"visual" | "raw">("raw");
  const [visualFields, setVisualFields] = useState<ConfigVisualField[] | null>(null);
  const [visualError, setVisualError] = useState<string | null>(null);
  const [textEditable, setTextEditable] = useState(true);
  const [rawDiagnostics, setRawDiagnostics] = useState<
    Array<{ message: string; line: number; column: number }>
  >([]);
  const rawEditorParent = useRef<HTMLDivElement | null>(null);
  const rawEditorView = useRef<EditorView | null>(null);
  const diagnoseTimer = useRef<number | null>(null);
  const diagnoseGeneration = useRef(0);
  const rawDiagnoseContext = useRef({ api, scope, agent, selection, file });
  const useCodeMirror = typeof navigator === "undefined" || !/jsdom/i.test(navigator.userAgent);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saveFeedback, setSaveFeedback] = useState<"idle" | "saving" | "saved">("idle");
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
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
  const fileLoadGeneration = useRef(0);
  const saveFeedbackTimer = useRef<number | null>(null);
  const unsavedTitleId = useId();
  const createTitleId = useId();
  const createHelpId = useId();
  const propagationTitleId = useId();
  const operationRunning = operation?.state === "running";
  const mutationBusy = busy || operationRunning;

  useEffect(() => {
    rawDiagnoseContext.current = { api, scope, agent, selection, file };
  }, [agent, api, file, scope, selection]);

  const scheduleRawDiagnose = useCallback((value: string) => {
    if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
    const generation = ++diagnoseGeneration.current;
    diagnoseTimer.current = window.setTimeout(() => {
      const {
        api: currentApi,
        scope: currentScope,
        agent: currentAgent,
        selection: currentSelection,
        file: currentFile,
      } = rawDiagnoseContext.current;
      if (!currentFile) return;
      void currentApi
        .post<{
          diagnostics: Array<{ severity?: string; message: string; line: number; column: number }>;
        }>("/_aibox/api/configs/diagnose", {
          ...scopeBody(currentScope),
          agent: currentAgent,
          current: currentSelection.current,
          config: currentSelection.current ? null : currentSelection.config,
          file: currentFile,
          content_base64: encodeBase64(new TextEncoder().encode(value)),
        })
        .then((result) => {
          if (diagnoseGeneration.current === generation)
            setRawDiagnostics(Array.isArray(result.diagnostics) ? result.diagnostics : []);
        })
        .catch(() => {
          if (diagnoseGeneration.current === generation) setRawDiagnostics([]);
        });
    }, 250);
  }, []);

  useEffect(
    () => () => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
    },
    [],
  );

  useEffect(
    () => () => {
      if (saveFeedbackTimer.current !== null) window.clearTimeout(saveFeedbackTimer.current);
    },
    [],
  );

  useEffect(() => {
    if (observedLocationVersion.current === locationVersion) return;
    observedLocationVersion.current = locationVersion;
    const route = readConfigRoute();
    setScope(route.scope);
    setAgent(route.agent);
    setSelection(route.selection);
    setFile(route.file);
    setDetailOpen(route.detailOpen);
    setSelectionMode(false);
    setSelectedNames(new Set());
  }, [locationVersion]);

  useEffect(() => {
    if (!detailOpen || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() =>
      (detailHeadingRef.current ?? detailBackButtonRef.current)?.focus(),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, file, selection]);

  const tenantOptions = useMemo<SessionFilterOption<ConfigScopeKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter((tenant): tenant is TenantRow & { kind: "managed"; name: string } =>
        Boolean(tenant.kind === "managed" && tenant.name),
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

  const agentOptions = useMemo<SessionFilterOption<Agent>[]>(
    () =>
      (["codex", "claude"] as const).map((value) => ({
        value,
        label: value === "codex" ? "Codex" : "Claude",
        icon: <AgentIcon agent={value} size={14} />,
      })),
    [],
  );
  const configTenantLabel =
    scope.scope === "host"
      ? "Host Tenant"
      : (tenants.find((tenant) => tenant.kind === "managed" && tenant.name === scope.tenant)
          ?.display_name ?? scope.tenant);
  const configSelectionLabel = selection.current
    ? "Current Config"
    : `Named Config ${selection.config}`;

  const loadCatalog = useCallback(
    async (kind: "initial" | "refresh" | "background" = "initial") => {
      catalogController.current?.abort();
      const controller = new AbortController();
      catalogController.current = controller;
      if (kind === "initial") setLoadingCatalog(true);
      if (kind === "refresh") setRefreshing(true);
      const query = scopeQuery(scope);
      query.set("agent", agent);
      try {
        const data = await api.get<ConfigListData>(
          `/_aibox/api/configs?${query}`,
          controller.signal,
        );
        if (controller.signal.aborted || catalogController.current !== controller) return null;
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
    [agent, api, scope],
  );

  useEffect(() => {
    setCatalog(null);
    setSnapshot(null);
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

  const editorBytes = useMemo(() => {
    if (!snapshot) return null;
    try {
      if (!textEditable) return null;
      return new TextEncoder().encode(editor);
    } catch {
      return null;
    }
  }, [editor, snapshot, textEditable]);
  const visualDirty =
    snapshot !== null &&
    visualFields !== null &&
    JSON.stringify(visualFields.map(({ path, included, value }) => ({ path, included, value }))) !==
      JSON.stringify(
        (snapshot.visual ?? []).map(({ path, included, value }) => ({ path, included, value })),
      );
  const editorDirty =
    snapshot !== null &&
    (editorMode === "visual"
      ? visualDirty
      : editorBytes !== null && encodeBase64(editorBytes) !== snapshot.content_base64);

  useEffect(() => onDirtyChange?.(editorDirty), [editorDirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);

  const setEditorFromSnapshot = useCallback(
    (value: ConfigFileData, preferredMode?: "visual" | "raw") => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) {
        window.clearTimeout(diagnoseTimer.current);
        diagnoseTimer.current = null;
      }
      const bytes = decodeBase64(value.content_base64);
      try {
        const content = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
        const mode = preferredMode ?? (value.visual ? "visual" : "raw");
        setEditor(content);
        setEditorMode(mode);
        setVisualFields(value.visual ?? null);
        setVisualError(value.visual_error ?? null);
        setTextEditable(true);
        setRawDiagnostics([]);
        if (mode === "raw") scheduleRawDiagnose(content);
      } catch {
        setEditor("");
        setEditorMode("raw");
        setVisualFields(null);
        setVisualError("This file is not valid UTF-8 and cannot be edited in the Console.");
        setTextEditable(false);
        setRawDiagnostics([]);
      }
    },
    [scheduleRawDiagnose],
  );

  useEffect(() => {
    if (!catalog || !file) {
      diagnoseGeneration.current += 1;
      setSnapshot(null);
      setEditor("");
      return;
    }
    const generation = ++fileLoadGeneration.current;
    diagnoseGeneration.current += 1;
    if (diagnoseTimer.current !== null) {
      window.clearTimeout(diagnoseTimer.current);
      diagnoseTimer.current = null;
    }
    setRawDiagnostics([]);
    setSnapshot(null);
    setEditor("");
    setLoadingFile(true);
    const body = {
      ...scopeBody(scope),
      agent,
      current: selection.current,
      config: selection.current ? null : selection.config,
      file,
    };
    void api
      .post<ConfigFileData>("/_aibox/api/configs/reveal", body)
      .then((value) => {
        if (fileLoadGeneration.current !== generation) return;
        setEditorFromSnapshot(value, selection.current ? "raw" : undefined);
        setSnapshot(value);
      })
      .catch((cause) => {
        if (fileLoadGeneration.current !== generation) return;
        setError(messageOf(cause));
      })
      .finally(() => {
        if (fileLoadGeneration.current === generation) setLoadingFile(false);
      });
    return () => {
      if (fileLoadGeneration.current === generation) fileLoadGeneration.current += 1;
    };
  }, [agent, api, catalog, file, scope, selection, setEditorFromSnapshot]);

  function switchEditorMode(next: "visual" | "raw") {
    if (next === editorMode) return;
    if (next === "visual") {
      if (!visualFields || visualError || rawDiagnostics.length > 0) {
        setError(
          visualError ??
            rawDiagnostics[0]?.message ??
            "Fix Raw Editor errors before switching to Visual.",
        );
        return;
      }
    }
    requestEditorAction(() => {
      if (next === "visual") {
        diagnoseGeneration.current += 1;
        if (diagnoseTimer.current !== null) {
          window.clearTimeout(diagnoseTimer.current);
          diagnoseTimer.current = null;
        }
      }
      setEditorMode(next);
      setError(null);
    });
  }

  function updateVisualField(path: string, update: Partial<ConfigVisualField>) {
    setVisualFields(
      (fields) =>
        fields?.map((field) => (field.path === path ? { ...field, ...update } : field)) ?? null,
    );
  }

  function visualPayload(): ConfigVisualFieldInput[] | undefined {
    if (editorMode !== "visual" || !visualFields) return undefined;
    return visualFields.map(({ path, included, value }) => ({ path, included, value }));
  }

  async function saveFile(refreshCatalog: boolean): Promise<boolean> {
    if (operationRunning || !snapshot || !file || editorBytes === null) return false;
    setBusy(true);
    setSaveFeedback("saving");
    if (saveFeedbackTimer.current !== null) window.clearTimeout(saveFeedbackTimer.current);
    try {
      const value = await api.post<ConfigFileData>("/_aibox/api/configs/save", {
        ...scopeBody(scope),
        agent,
        current: selection.current,
        config: selection.current ? null : selection.config,
        file,
        revision: snapshot.revision,
        content_base64: encodeBase64(editorBytes),
        visual: visualPayload(),
      });
      setEditorFromSnapshot(value, editorMode);
      setSnapshot(value);
      setError(null);
      if (refreshCatalog) await loadCatalog("background");
      setSaveFeedback("saved");
      saveFeedbackTimer.current = window.setTimeout(() => {
        setSaveFeedback("idle");
        saveFeedbackTimer.current = null;
      }, 4_000);
      return true;
    } catch (cause) {
      setSaveFeedback("idle");
      setError(messageOf(cause));
      return false;
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (editorDirty && saveFeedback === "saved") setSaveFeedback("idle");
  }, [editorDirty, saveFeedback]);

  useEffect(() => {
    if (
      !useCodeMirror ||
      !rawEditorParent.current ||
      editorMode !== "raw" ||
      !snapshot ||
      !textEditable
    )
      return;
    rawEditorView.current?.destroy();
    const language: Extension = file?.endsWith(".json") ? json() : StreamLanguage.define(toml);
    const view = new EditorView({
      parent: rawEditorParent.current,
      state: EditorState.create({
        doc: editor,
        extensions: [
          basicSetup,
          language,
          EditorView.cspNonce.of(codeMirrorCspNonce()),
          syntaxHighlighting(configHighlightStyle),
          lintGutter(),
          keymap.of([...defaultKeymap, indentWithTab, ...searchKeymap]),
          EditorView.contentAttributes.of({ "aria-label": `${file} content` }),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            const value = update.state.doc.toString();
            setEditor(value);
            scheduleRawDiagnose(value);
          }),
        ],
      }),
    });
    rawEditorView.current = view;
    return () => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) {
        window.clearTimeout(diagnoseTimer.current);
        diagnoseTimer.current = null;
      }
      view.destroy();
      rawEditorView.current = null;
    };
    // The instance is recreated only when the selected file or editor mode changes.
    // The document is synchronized below so typing never tears down the view.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editorMode, file, scheduleRawDiagnose, snapshot, textEditable, useCodeMirror]);

  useEffect(() => {
    const view = rawEditorView.current;
    if (!useCodeMirror) return;
    if (!view || editorMode !== "raw") return;
    const current = view.state.doc.toString();
    if (current === editor) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: editor },
    });
  }, [editor, editorMode, useCodeMirror]);

  useEffect(() => {
    const view = rawEditorView.current;
    if (!useCodeMirror) return;
    if (!view || editorMode !== "raw") return;
    const diagnostics = rawDiagnostics.map((diagnostic) => {
      const line = Math.max(1, diagnostic.line);
      const column = Math.max(1, diagnostic.column);
      const lineInfo = view.state.doc.line(Math.min(line, view.state.doc.lines));
      const from = Math.min(lineInfo.from + column - 1, lineInfo.to);
      return {
        from,
        to: Math.min(from + 1, lineInfo.to),
        severity: "error" as const,
        message: diagnostic.message,
      };
    });
    view.dispatch(setDiagnostics(view.state, diagnostics));
  }, [rawDiagnostics, editorMode, useCodeMirror]);

  function restoreSnapshot() {
    if (!snapshot) return;
    setEditorFromSnapshot(snapshot);
    setError(null);
  }

  function requestEditorAction(run: () => void | Promise<void>) {
    if (editorDirty) setPendingAction({ run });
    else void run();
  }

  async function saveAndRunPendingAction() {
    if (!pendingAction) return;
    const action = pendingAction.run;
    if (!(await saveFile(false))) return;
    setPendingAction(null);
    await action();
  }

  async function discardAndRunPendingAction() {
    if (!pendingAction) return;
    const action = pendingAction.run;
    restoreSnapshot();
    setPendingAction(null);
    await action();
  }

  function selectScope(values: ReadonlySet<ConfigScopeKey>) {
    const next = [...values][0];
    if (!next || next === configScopeKey(scope)) return;
    requestEditorAction(() => {
      setScope(scopeFromConfigKey(next));
      setSelection({ current: true });
      setSelectionMode(false);
      setSelectedNames(new Set());
      setDetailOpen(false);
      changePageLocation(
        "configs",
        configLocation(scopeFromConfigKey(next), agent, null),
        onLocationChange,
      );
    });
  }

  function selectAgent(values: ReadonlySet<Agent>) {
    const next = [...values][0];
    if (!next || next === agent) return;
    requestEditorAction(() => {
      setAgent(next);
      setSelection({ current: true });
      setSelectionMode(false);
      setSelectedNames(new Set());
      setDetailOpen(false);
      changePageLocation("configs", configLocation(scope, next, null), onLocationChange);
    });
  }

  function openConfig(name: string) {
    requestEditorAction(() => {
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      const nextSelection: ConfigSelection = { current: false, config: name };
      changePageLocation(
        "configs",
        configLocation(scope, agent, nextSelection, file),
        onLocationChange,
      );
    });
  }

  function openCurrent() {
    requestEditorAction(() => {
      setSelection({ current: true });
      setDetailOpen(true);
      changePageLocation(
        "configs",
        configLocation(scope, agent, { current: true }, file),
        onLocationChange,
      );
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
      await api.post("/_aibox/api/configs/create", { ...scopeBody(scope), agent, config: name });
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadCatalog("background");
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      changePageLocation(
        "configs",
        configLocation(scope, agent, { current: false, config: name }, file),
        onLocationChange,
      );
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
      await api.post("/_aibox/api/configs/apply", { ...scopeBody(scope), agent, config: name });
    } catch (cause) {
      applyError = `${messageOf(cause)} Some Current Config files may already have been updated.`;
    } finally {
      const refreshed = await loadCatalog("background");
      if (refreshed) {
        await Promise.allSettled(
          refreshed.files.map((currentFile) =>
            api.post<ConfigFileData>("/_aibox/api/configs/reveal", {
              ...scopeBody(scope),
              agent,
              current: true,
              config: null,
              file: currentFile,
            }),
          ),
        );
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
      await api.post("/_aibox/api/configs/delete", {
        ...scopeBody(scope),
        agent,
        configs: requestedNames,
        all: false,
        confirmation: requestedNames.length === 1 ? requestedNames[0] : "",
      });
      const deletedSelected = !selection.current && requestedNames.includes(selection.config ?? "");
      setDeleteTarget(null);
      setSelectionMode(false);
      setSelectedNames(new Set());
      if (deletedSelected) {
        setSelection({ current: true });
        setDetailOpen(false);
        changePageLocation("configs", configLocation(scope, agent, null), onLocationChange, true);
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
          changePageLocation("configs", configLocation(scope, agent, null), onLocationChange, true);
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
      setPreview(await api.post<PropagationPreview>("/_aibox/api/configs/propagate-auth/preview"));
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
      setReport(
        await api.post<PropagationReport>("/_aibox/api/configs/propagate-auth/execute", {
          plan_id: preview.plan_id,
        }),
      );
      setPreview(null);
      await loadCatalog("background");
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  const createNameValid = CONFIG_NAME_PATTERN.test(newName);
  const propagationHasFailures =
    report?.entries.some((entry) => entry.outcome.status === "failed") ?? false;
  const propagationNeedsAttention =
    report?.entries.some((entry) => propagationGroup(entry.outcome.status) === "attention") ??
    false;

  return (
    <div className={`${styles.page} ${styles.configPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={
          tenantError
            ? retryTenants
            : error
              ? () => {
                  setError(null);
                  void loadCatalog("refresh");
                }
              : undefined
        }
      />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.configLayout} ${detailOpen ? styles.configDetailOpen : ""}`}>
        <aside className={styles.configCatalog} aria-label="Configs">
          <div className={styles.sessionToolbar}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={styles.sessionCancelSelection}
                  disabled={busy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={styles.sessionSelectionActions}>
                  <span className={styles.sessionSelectionCount}>{selectedCount} selected</span>
                  <button
                    type="button"
                    className={styles.sessionSelectAll}
                    disabled={selectableNames.length === 0 || busy}
                    onClick={toggleAllConfigs}
                  >
                    {allSelectable ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={styles.sessionDeleteSelected}
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
                <div className={styles.sessionFilters}>
                  <SessionMultiSelect
                    className={styles.sessionTenantFilter}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Tenant"
                    onCommit={selectScope}
                    options={tenantOptions}
                    pluralLabel="tenants"
                    selected={new Set([configScopeKey(scope)])}
                    triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                    allowMultiple={false}
                  />
                  <SessionMultiSelect
                    className={styles.sessionAgentFilter}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Coding Agent"
                    onCommit={selectAgent}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={new Set([agent])}
                    triggerIcon={<AgentIcon agent={agent} size={14} />}
                    allowMultiple={false}
                  />
                </div>
                <div className={styles.sessionHeaderActions}>
                  <IconButton
                    className={styles.sessionRefresh}
                    label={refreshing ? "Refreshing Configs" : "Refresh Configs"}
                    aria-busy={refreshing}
                    disabled={loadingCatalog || refreshing || busy}
                    onClick={() =>
                      requestEditorAction(async () => {
                        await loadCatalog("refresh");
                      })
                    }
                  >
                    <RefreshCw className={refreshing ? "spin" : undefined} size={14} />
                  </IconButton>
                  <button
                    type="button"
                    className={styles.sessionSelect}
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
          <div className={styles.configList} aria-busy={loadingCatalog}>
            {(loadingTenants || loadingCatalog) && !catalog && <Loading />}
            <div className={styles.configRowGroup}>
              <div
                className={`${styles.configRow} ${selection.current ? styles.configRowInspected : ""} ${selectionMode ? `${styles.configRowSelection} ${styles.configRowProtected}` : ""}`}
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
                  {selectionMode && <span className={styles.configProtected}>Protected</span>}
                </button>
                {!selectionMode &&
                  scope.scope === "host" &&
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
              <div className={styles.catalogDivider}>
                <span>Named Configs</span>
                <IconButton
                  className={styles.configAddButton}
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
                const issue = configIssuePresentation(entry);
                const issueDescriptionId = issue
                  ? configIssueDescriptionId(scope, agent, entry.name)
                  : undefined;
                return (
                  <div
                    key={entry.name}
                    className={`${styles.configRow} ${selectedForInspection ? styles.configRowInspected : ""} ${selectedForDeletion ? styles.configRowSelected : ""} ${selectionMode ? styles.configRowSelection : ""}`}
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
                        <span className={styles.sessionSelectionIndicator} aria-hidden="true">
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
                      <div className={styles.configRowActions}>
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
                          className={`${styles.configRowAction} ${styles.configDeleteAction}`}
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
        <section className={styles.configEditor}>
          {catalog ? (
            <>
              <div className={styles.configEditorHeader}>
                <IconButton
                  buttonRef={detailBackButtonRef}
                  label="Back to Configs"
                  onClick={() =>
                    requestEditorAction(() => {
                      const focusKey = selection.current ? "current" : selection.config;
                      setDetailOpen(false);
                      changePageLocation(
                        "configs",
                        configLocation(scope, agent, null),
                        onLocationChange,
                      );
                      window.requestAnimationFrame(() => {
                        if (focusKey) configRowButtons.current.get(focusKey)?.focus();
                      });
                    })
                  }
                >
                  <ChevronLeft size={17} />
                </IconButton>
                <div className={styles.configContextStack}>
                  <div className={styles.contextFacts} aria-label="Config editing context">
                    <span>
                      <small>Scope</small>
                      <strong>
                        {configTenantLabel}
                        {scope.scope === "host" && <em>Host risk</em>}
                      </strong>
                    </span>
                    <span>
                      <small>Agent</small>
                      <strong>{agent === "codex" ? "Codex" : "Claude"}</strong>
                    </span>
                    <span>
                      <small>Config</small>
                      <strong>{configSelectionLabel}</strong>
                    </span>
                    <span>
                      <small>File</small>
                      <strong className={styles.contextFile}>{file ?? "—"}</strong>
                    </span>
                  </div>
                  {(selection.current || file === "auth.json" || editorMode === "raw") && (
                    <span className={styles.sensitiveContext}>
                      Native content may contain credentials and is displayed without redaction.
                    </span>
                  )}
                  {catalog.files.length > 1 ? (
                    <div className={styles.fileTabs} role="tablist" aria-label="Config files">
                      {catalog.files.map((name, index) => (
                        <button
                          type="button"
                          id={`config-file-tab-${name.replace(/[^a-zA-Z0-9_-]/g, "-")}`}
                          role="tab"
                          aria-selected={file === name}
                          aria-controls="config-file-panel"
                          tabIndex={file === name ? 0 : -1}
                          key={name}
                          onKeyDown={(event) => {
                            const last = catalog.files.length - 1;
                            let next: number;
                            if (event.key === "ArrowRight") next = index === last ? 0 : index + 1;
                            else if (event.key === "ArrowLeft")
                              next = index === 0 ? last : index - 1;
                            else if (event.key === "Home") next = 0;
                            else if (event.key === "End") next = last;
                            else return;
                            event.preventDefault();
                            const nextFile = catalog.files[next];
                            requestEditorAction(() => {
                              setFile(nextFile);
                              changePageLocation(
                                "configs",
                                configLocation(scope, agent, selection, nextFile),
                                onLocationChange,
                              );
                              window.requestAnimationFrame(() =>
                                document
                                  .getElementById(
                                    `config-file-tab-${nextFile.replace(/[^a-zA-Z0-9_-]/g, "-")}`,
                                  )
                                  ?.focus(),
                              );
                            });
                          }}
                          onClick={() =>
                            requestEditorAction(() => {
                              setFile(name);
                              changePageLocation(
                                "configs",
                                configLocation(scope, agent, selection, name),
                                onLocationChange,
                              );
                            })
                          }
                        >
                          {name}
                        </button>
                      ))}
                    </div>
                  ) : (
                    <h2 ref={detailHeadingRef} tabIndex={-1}>
                      {file ?? "Configuration"}
                    </h2>
                  )}
                </div>
              </div>
              <div
                id="config-file-panel"
                className={styles.configFilePanel}
                role="tabpanel"
                aria-labelledby={
                  file && catalog.files.length > 1
                    ? `config-file-tab-${file.replace(/[^a-zA-Z0-9_-]/g, "-")}`
                    : undefined
                }
              >
                {loadingFile ? (
                  <Loading />
                ) : snapshot ? (
                  <>
                    <div className={styles.editorTools}>
                      <span>{snapshot.exists ? "Existing file" : "New file"}</span>
                      <div className={styles.segmented} aria-label="Editor mode">
                        {visualFields && (
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
                      <button
                        className={styles.primaryButton}
                        type="button"
                        disabled={
                          mutationBusy ||
                          !editorDirty ||
                          (editorMode === "raw" && editorBytes === null)
                        }
                        onClick={() => void saveFile(true)}
                      >
                        {saveFeedback === "saving" ? (
                          <LoaderCircle className="spin" size={14} aria-hidden="true" />
                        ) : (
                          <Save size={14} />
                        )}
                        <span aria-live="polite">
                          {saveFeedback === "saving"
                            ? "Saving…"
                            : saveFeedback === "saved"
                              ? "Saved"
                              : "Save"}
                        </span>
                      </button>
                    </div>
                    {editorMode === "visual" && visualFields ? (
                      <VisualConfigFields fields={visualFields} onChange={updateVisualField} />
                    ) : textEditable ? (
                      useCodeMirror ? (
                        <div
                          ref={rawEditorParent}
                          className={styles.codeEditor}
                          aria-label={`${file} content`}
                        />
                      ) : (
                        <textarea
                          className={`${styles.codeEditor} ${styles.codeEditorFallback}`}
                          aria-label={`${file} content`}
                          value={editor}
                          onChange={(event) => {
                            const value = event.target.value;
                            setEditor(value);
                            scheduleRawDiagnose(value);
                          }}
                          spellCheck={false}
                        />
                      )
                    ) : (
                      <div className={styles.binaryConfigNotice} role="status">
                        <AlertTriangle size={18} aria-hidden="true" />
                        <span>
                          This file is not valid UTF-8 and cannot be edited in the Console.
                        </span>
                        <button
                          type="button"
                          onClick={() => {
                            const bytes = decodeBase64(snapshot.content_base64);
                            const copy = new Uint8Array(bytes);
                            const url = URL.createObjectURL(
                              new Blob([copy.buffer], { type: "application/octet-stream" }),
                            );
                            const link = document.createElement("a");
                            link.href = url;
                            link.download = file ?? "config";
                            link.click();
                            URL.revokeObjectURL(url);
                          }}
                        >
                          <Download size={14} /> Download raw file
                        </button>
                      </div>
                    )}
                    {editorMode === "raw" && rawDiagnostics.length > 0 && (
                      <div className={styles.editorDiagnostics} role="alert">
                        {rawDiagnostics.map((diagnostic, index) => (
                          <span key={`${diagnostic.line}-${diagnostic.column}-${index}`}>
                            Line {diagnostic.line}, column {diagnostic.column}: {diagnostic.message}
                          </span>
                        ))}
                      </div>
                    )}
                  </>
                ) : (
                  <div className={styles.emptyPane}>
                    <NamedConfigIcon size={22} />
                    <span>Unable to load {file ?? "configuration"}.</span>
                  </div>
                )}
              </div>
            </>
          ) : loadingTenants || loadingCatalog ? (
            <Loading />
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
          className={styles.dialog}
          ariaLabelledBy={unsavedTitleId}
          busy={mutationBusy}
          onCancel={() => setPendingAction(null)}
        >
          <section>
            <h2 id={unsavedTitleId}>Unsaved changes</h2>
            <p>Save changes to {file ?? "this file"} before continuing?</p>
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
              <button
                className={styles.primaryButton}
                type="button"
                onClick={() => void saveAndRunPendingAction()}
                disabled={mutationBusy || editorBytes === null}
              >
                Save and continue
              </button>
            </div>
          </section>
        </Dialog>
      )}
      {createOpen && (
        <Dialog
          className={styles.dialog}
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
              <input
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
            <p id={createHelpId} className={styles.dialogDescription}>
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
              <button
                className={styles.primaryButton}
                type="submit"
                disabled={!createNameValid || mutationBusy}
              >
                {busy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Plus size={14} />
                )}
                {busy ? "Creating…" : "Create"}
              </button>
            </div>
          </form>
        </Dialog>
      )}
      {applyTarget && (
        <ConfirmDialog
          title={`Apply Named Config ${applyTarget.name} to Current Config?`}
          description={
            <div className={styles.dialogDescription}>
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
          confirmation={scope.scope === "host" ? "Host Tenant" : undefined}
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
            <p className={styles.dialogDescription}>
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
              <p className={styles.dialogDescription}>
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
          className={`${styles.dialog} ${styles.wideDialog}`}
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
                <button
                  className={styles.primaryButton}
                  type="button"
                  disabled={mutationBusy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  {busy && <LoaderCircle className="spin" size={14} aria-hidden="true" />}
                  {busy
                    ? "Propagating…"
                    : `Propagate ${preview.preview.updates} credential update${preview.preview.updates === 1 ? "" : "s"}`}
                </button>
              )}
            </div>
          </section>
        </Dialog>
      )}
    </div>
  );
}

function ConfigDriftBadge({ status }: { status: ApplicationStatus }) {
  const driftLabel =
    status.drift === "comparison-error"
      ? "Comparison error"
      : status.drift === "source-missing"
        ? "Source missing"
        : status.drift[0].toUpperCase() + status.drift.slice(1);
  return (
    <span
      className={`${styles.configDriftBadge} ${
        status.drift === "clean"
          ? styles.goodStatus
          : status.drift === "untracked"
            ? styles.neutralStatus
            : styles.warnStatus
      }`}
      title={status.detail ?? status.last_application?.applied_at}
    >
      {driftLabel}
    </span>
  );
}

type SessionScopeKey = "host" | `managed:${string}`;

interface SessionSource {
  key: string;
  scope: Scope;
  scopeKey: SessionScopeKey;
  scopeLabel: string;
  agent: Agent;
  agentLabel: string;
}

interface SourcedSession extends SessionRow {
  key: string;
  source: SessionSource;
}

interface AggregatedSessionData {
  sessions: SourcedSession[];
  warnings: string[];
  partial: boolean;
}

interface SessionFilterOption<T extends string> {
  value: T;
  label: string;
  summaryLabel?: string;
  icon: ReactNode;
}

type SessionDeletion = { kind: "record"; key: string } | { kind: "batch" } | null;

const SESSION_AGENT_OPTIONS: readonly { value: Agent; label: string }[] = [
  { value: "codex", label: "Codex" },
  { value: "claude", label: "Claude" },
];

function agentLabel(agent: Agent): string {
  return SESSION_AGENT_OPTIONS.find((option) => option.value === agent)?.label ?? agent;
}

function scopeFromSessionKey(key: SessionScopeKey): Scope {
  return key === "host" ? { scope: "host" } : { scope: "managed", tenant: key.slice(8) };
}

function sessionScopeLabel(key: SessionScopeKey): string {
  return key === "host" ? "Host Tenant" : `Tenant ${key.slice(8)}`;
}

function sessionListScopeLabel(key: SessionScopeKey): string {
  return key === "host" ? "Host Tenant" : key.slice(8);
}

function sessionSource(scopeKey: SessionScopeKey, agent: Agent): SessionSource {
  return {
    key: JSON.stringify([scopeKey, agent]),
    scope: scopeFromSessionKey(scopeKey),
    scopeKey,
    scopeLabel: sessionScopeLabel(scopeKey),
    agent,
    agentLabel: agentLabel(agent),
  };
}

interface SessionRouteSelection {
  scopeKey: SessionScopeKey;
  agent: Agent;
  id: string;
}

interface SessionRouteState {
  scopes: Set<SessionScopeKey>;
  agents: Set<Agent>;
  selection: SessionRouteSelection | null;
}

function readSessionRoute(): SessionRouteState {
  const query = pageSearch();
  const scopes = new Set(
    query
      .getAll("scope")
      .map(tenantKeyFromParam)
      .filter((value): value is SessionScopeKey => value !== null),
  );
  const agents = new Set(
    query
      .getAll("agent")
      .filter((value): value is Agent => value === "codex" || value === "claude"),
  );
  if (scopes.size === 0) scopes.add("managed:default");
  if (agents.size === 0) agents.add("codex");

  const selectedScope = tenantKeyFromParam(query.get("session_scope"));
  const selectedAgent = query.get("session_agent");
  const id = query.get("session");
  const selection: SessionRouteSelection | null =
    selectedScope && (selectedAgent === "codex" || selectedAgent === "claude") && id
      ? { scopeKey: selectedScope, agent: selectedAgent, id }
      : null;
  return { scopes, agents, selection };
}

function sessionLocation(
  scopes: ReadonlySet<SessionScopeKey>,
  agents: ReadonlySet<Agent>,
  selection?: SessionRouteSelection | null,
): URLSearchParams {
  const query = new URLSearchParams();
  for (const scope of [...scopes].sort()) query.append("scope", scope);
  for (const agent of SESSION_AGENT_OPTIONS.map((option) => option.value)) {
    if (agents.has(agent)) query.append("agent", agent);
  }
  if (selection) {
    query.set("session_scope", selection.scopeKey);
    query.set("session_agent", selection.agent);
    query.set("session", selection.id);
  }
  return query;
}

function sourcedSession(source: SessionSource, row: SessionRow): SourcedSession {
  return {
    ...row,
    key: JSON.stringify([source.scopeKey, source.agent, row.id]),
    source,
  };
}

function compareSessions(left: SourcedSession, right: SourcedSession): number {
  return (
    right.start_ts.localeCompare(left.start_ts) ||
    left.source.scopeLabel.localeCompare(right.source.scopeLabel) ||
    left.source.agentLabel.localeCompare(right.source.agentLabel) ||
    left.id.localeCompare(right.id)
  );
}

function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

function focusTargetAfterSessionDelete(rows: SourcedSession[], key: string): string | null {
  const index = rows.findIndex((row) => row.key === key);
  if (index < 0) return null;
  return rows[index + 1]?.key ?? rows[index - 1]?.key ?? null;
}

function SessionMultiSelect<T extends string>({
  allowMultiple = true,
  className,
  disabled,
  label,
  onCommit,
  options,
  pluralLabel,
  selected,
  triggerIcon,
}: {
  allowMultiple?: boolean;
  className?: string;
  disabled: boolean;
  label: string;
  onCommit: (values: ReadonlySet<T>) => void;
  options: readonly SessionFilterOption<T>[];
  pluralLabel: string;
  selected: ReadonlySet<T>;
  triggerIcon: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<"single" | "multiple" | "choose-one">("single");
  const [draft, setDraft] = useState<Set<T>>(() => new Set(selected));
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedOption = options.find((option) => selected.has(option.value));
  const summary =
    selected.size === 1
      ? (selectedOption?.summaryLabel ?? selectedOption?.label ?? "1 selected")
      : `${selected.size} ${pluralLabel}`;
  const draftChanged =
    draft.size !== selected.size || [...draft].some((value) => !selected.has(value));
  const singleSelectedValue = selected.size === 1 ? [...selected][0] : undefined;
  const singleFocusIndex = Math.max(
    0,
    options.findIndex((option) => option.value === singleSelectedValue),
  );
  const multiFocusIndex = Math.max(
    0,
    options.findIndex((option) => draft.has(option.value)),
  );

  function openMenu() {
    setDraft(new Set(selected));
    setMode(allowMultiple && selected.size > 1 ? "multiple" : "single");
    setOpen(true);
  }

  useEffect(() => {
    if (!open) return;
    function closeOnOutsidePointer(event: PointerEvent) {
      if (rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    }

    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [open]);

  function closeAndFocusTrigger() {
    setOpen(false);
    triggerRef.current?.focus();
  }

  function commitOnly(value: T) {
    if (selected.size !== 1 || !selected.has(value)) onCommit(new Set([value]));
    closeAndFocusTrigger();
  }

  function toggleDraft(value: T) {
    setDraft((current) => {
      if (current.has(value) && current.size === 1) return current;
      const next = new Set(current);
      if (!next.delete(value)) next.add(value);
      return next;
    });
  }

  function applyDraft() {
    if (!draftChanged) return;
    onCommit(new Set(draft));
    closeAndFocusTrigger();
  }

  function handleSingleOptionKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number | null = null;
    if (event.key === "ArrowDown") nextIndex = (index + 1) % options.length;
    if (event.key === "ArrowUp") nextIndex = (index - 1 + options.length) % options.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = options.length - 1;
    if (event.key === "Escape") {
      event.preventDefault();
      closeAndFocusTrigger();
      return;
    }
    if (nextIndex === null) return;
    event.preventDefault();
    optionRefs.current[nextIndex]?.focus();
  }

  return (
    <div
      ref={rootRef}
      className={`${styles.sessionFilter} ${className ?? ""}`}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.preventDefault();
        closeAndFocusTrigger();
      }}
    >
      <button
        ref={triggerRef}
        type="button"
        className={styles.sessionFilterTrigger}
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={`${label}: ${summary}`}
        title={`${label}: ${summary}`}
        disabled={disabled}
        onClick={() => {
          if (open) setOpen(false);
          else openMenu();
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          if (!open) openMenu();
        }}
      >
        <span className={styles.sessionFilterTriggerIcon}>{triggerIcon}</span>
        <span className={styles.sessionFilterTriggerSummary}>
          {selected.size === 1 ? (
            summary
          ) : (
            <>
              <span className={styles.sessionFilterSummaryFull}>{summary}</span>
              <span className={styles.sessionFilterSummaryCompact} aria-hidden="true">
                {selected.size}
              </span>
            </>
          )}
        </span>
        <ChevronDown
          className={open ? styles.sessionFilterChevronOpen : undefined}
          size={13}
          aria-hidden="true"
        />
      </button>
      {open && (
        <div id={menuId} className={styles.sessionFilterMenu} role="dialog" aria-label={label}>
          {mode === "choose-one" && (
            <div className={styles.sessionFilterMenuHeader}>
              <button
                type="button"
                aria-label={`Back to multiple ${pluralLabel}`}
                onClick={() => setMode("multiple")}
              >
                <ChevronLeft size={13} aria-hidden="true" />
                Back
              </button>
            </div>
          )}
          {mode === "multiple" ? (
            <div className={styles.sessionFilterOptions} role="group" aria-label={pluralLabel}>
              {options.map((option, index) => {
                const checked = draft.has(option.value);
                return (
                  <label
                    className={`${styles.sessionFilterOption} ${styles.sessionFilterOptionMultiple}`}
                    key={option.value}
                    title={option.label}
                  >
                    <input
                      autoFocus={index === multiFocusIndex}
                      type="checkbox"
                      checked={checked}
                      disabled={checked && draft.size === 1}
                      onChange={() => toggleDraft(option.value)}
                    />
                    <span className={styles.sessionFilterOptionIcon}>{option.icon}</span>
                    <span className={styles.sessionFilterOptionLabel}>{option.label}</span>
                  </label>
                );
              })}
            </div>
          ) : (
            <div
              className={styles.sessionFilterOptions}
              role="listbox"
              aria-label={`${label} single selection`}
            >
              {options.map((option, index) => {
                const active = mode === "single" && option.value === singleSelectedValue;
                return (
                  <button
                    autoFocus={index === singleFocusIndex}
                    type="button"
                    role="option"
                    aria-selected={active}
                    className={`${styles.sessionFilterOption} ${styles.sessionFilterOptionSingle} ${
                      active ? styles.sessionFilterOptionSelected : ""
                    }`}
                    key={option.value}
                    ref={(element) => {
                      optionRefs.current[index] = element;
                    }}
                    title={option.label}
                    onClick={() => commitOnly(option.value)}
                    onKeyDown={(event) => handleSingleOptionKeyDown(event, index)}
                  >
                    <span className={styles.sessionFilterOptionIcon}>{option.icon}</span>
                    <span className={styles.sessionFilterOptionLabel}>{option.label}</span>
                    <span className={styles.sessionFilterOptionCheckSlot} aria-hidden="true">
                      {active && <Check className={styles.sessionFilterOptionCheck} size={14} />}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          {mode === "single" && allowMultiple && (
            <div className={styles.sessionFilterMenuFooter}>
              <button
                type="button"
                className={styles.sessionFilterModeAction}
                aria-label={`Select multiple ${pluralLabel}`}
                onClick={() => {
                  setDraft(new Set(selected));
                  setMode("multiple");
                }}
              >
                <ListChecks size={13} aria-hidden="true" />
                Select multiple
              </button>
            </div>
          )}
          {mode === "multiple" && (
            <div
              className={`${styles.sessionFilterMenuFooter} ${styles.sessionFilterMenuFooterMultiple}`}
            >
              <button
                type="button"
                className={styles.sessionFilterModeAction}
                aria-label={`Choose one ${label}`}
                onClick={() => setMode("choose-one")}
              >
                Choose one
              </button>
              <div className={styles.sessionFilterCommitActions}>
                <button type="button" onClick={closeAndFocusTrigger}>
                  Cancel
                </button>
                <button type="button" disabled={!draftChanged} onClick={applyDraft}>
                  Apply
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function SessionPage({ api, operation, locationVersion = 0, onLocationChange }: PageProps) {
  const [initialRoute] = useState(readSessionRoute);
  const observedLocationVersion = useRef(locationVersion);
  const {
    tenants,
    loading: loadingTenants,
    error: tenantError,
    retry: retryTenants,
  } = useTenants(api);
  const [selectedScopes, setSelectedScopes] = useState<Set<SessionScopeKey>>(
    () => initialRoute.scopes,
  );
  const [selectedAgents, setSelectedAgents] = useState<Set<Agent>>(() => initialRoute.agents);
  const [routeSelection, setRouteSelection] = useState<SessionRouteSelection | null>(
    initialRoute.selection,
  );
  const [data, setData] = useState<AggregatedSessionData | null>(null);
  const [currentSession, setCurrentSession] = useState<SourcedSession | null>(null);
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [promptWarnings, setPromptWarnings] = useState<string[]>([]);
  const [loadingPrompts, setLoadingPrompts] = useState(false);
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [dialogKeys, setDialogKeys] = useState<string[] | null>(null);
  const [singleDeleteTarget, setSingleDeleteTarget] = useState<SourcedSession | null>(null);
  const [deletion, setDeletion] = useState<SessionDeletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [listUnavailable, setListUnavailable] = useState(false);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const listController = useRef<AbortController | null>(null);
  const streamController = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SourcedSession | null>(null);
  const deletionInFlight = useRef(false);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const sessionRowButtons = useRef(new Map<string, HTMLButtonElement>());
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();

  useEffect(() => {
    if (!currentSession || !window.matchMedia?.("(max-width: 760px)").matches) return;
    const frame = window.requestAnimationFrame(() => detailHeadingRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [currentSession]);

  const tenantOptions = useMemo<SessionFilterOption<SessionScopeKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter((tenant): tenant is TenantRow & { kind: "managed"; name: string } =>
        Boolean(tenant.kind === "managed" && tenant.name),
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

  const agentOptions = useMemo<SessionFilterOption<Agent>[]>(
    () =>
      SESSION_AGENT_OPTIONS.map((option) => ({
        ...option,
        icon: <AgentIcon agent={option.value} size={14} />,
      })),
    [],
  );

  const sources = useMemo(() => {
    const scopeKeys = [...selectedScopes].sort();
    const agents = SESSION_AGENT_OPTIONS.map((option) => option.value).filter((agent) =>
      selectedAgents.has(agent),
    );
    return scopeKeys.flatMap((scopeKey) =>
      agents.map((selectedAgent) => sessionSource(scopeKey, selectedAgent)),
    );
  }, [selectedAgents, selectedScopes]);

  const abortPromptStream = useCallback(() => {
    streamController.current?.abort();
    streamController.current = null;
    setLoadingPrompts(false);
  }, []);

  const clearInspection = useCallback(() => {
    abortPromptStream();
    currentSessionRef.current = null;
    setCurrentSession(null);
    setPrompts([]);
    setPromptWarnings([]);
  }, [abortPromptStream]);

  const openSession = useCallback(
    async (row: SourcedSession, updateLocation = true) => {
      abortPromptStream();
      const controller = new AbortController();
      streamController.current = controller;
      currentSessionRef.current = row;
      setCurrentSession(row);
      setPrompts([]);
      setPromptWarnings([]);
      setLoadingPrompts(true);
      if (updateLocation) {
        const nextSelection = {
          scopeKey: row.source.scopeKey,
          agent: row.source.agent,
          id: row.id,
        };
        setRouteSelection(nextSelection);
        changePageLocation(
          "sessions",
          sessionLocation(selectedScopes, selectedAgents, nextSelection),
          onLocationChange,
        );
      }
      const query = scopeQuery(row.source.scope);
      query.set("agent", row.source.agent);
      query.set("id", row.id);
      try {
        const result = await api.streamSession(
          `/_aibox/api/sessions/prompts?${query}`,
          (prompt) => setPrompts((current) => [...current, prompt]),
          controller.signal,
        );
        if (streamController.current === controller) setPromptWarnings(result.warnings);
      } catch (cause) {
        if (!sessionRequestCancelled(cause, controller.signal)) {
          setError(
            `Couldn’t load Session from ${row.source.scopeLabel} · ${row.source.agentLabel}: ${messageOf(cause)}`,
          );
        }
      } finally {
        if (streamController.current === controller) {
          streamController.current = null;
          setLoadingPrompts(false);
        }
      }
    },
    [abortPromptStream, api, onLocationChange, selectedAgents, selectedScopes],
  );

  useEffect(() => {
    if (observedLocationVersion.current === locationVersion) return;
    observedLocationVersion.current = locationVersion;
    const route = readSessionRoute();
    clearInspection();
    setData(null);
    setSelectedScopes(route.scopes);
    setSelectedAgents(route.agents);
    setRouteSelection(route.selection);
  }, [clearInspection, locationVersion]);

  const load = useCallback(
    async (kind: "initial" | "refresh" = "initial"): Promise<AggregatedSessionData | null> => {
      listController.current?.abort();
      const controller = new AbortController();
      listController.current = controller;
      if (kind === "refresh") {
        setLoadingList(false);
        setRefreshing(true);
      } else {
        setRefreshing(false);
        setLoadingList(true);
      }
      try {
        const results = await Promise.allSettled(
          sources.map(async (source) => {
            const query = scopeQuery(source.scope);
            query.set("agent", source.agent);
            const result = await api.get<SessionListData>(
              `/_aibox/api/sessions?${query}`,
              controller.signal,
            );
            return { result, source };
          }),
        );
        if (listController.current !== controller || controller.signal.aborted) return null;

        const failures = results.flatMap((result, index) =>
          result.status === "rejected"
            ? [{ cause: result.reason as unknown, source: sources[index] }]
            : [],
        );
        const successes = results.flatMap((result) =>
          result.status === "fulfilled" ? [result.value] : [],
        );
        if (successes.length === 0 && failures.length > 0) {
          const failureText = failures
            .map(
              ({ cause, source }) =>
                `${source.scopeLabel} · ${source.agentLabel}: ${messageOf(cause)}`,
            )
            .join("; ");
          setListUnavailable(true);
          setError(`Couldn’t load Sessions: ${failureText}`);
          setData((current) =>
            kind === "refresh" && current ? current : { sessions: [], warnings: [], partial: true },
          );
          setSelectionMode(false);
          setSelectedKeys(new Set());
          return null;
        }

        const warnings = [
          ...failures.map(
            ({ cause, source }) =>
              `${source.scopeLabel} · ${source.agentLabel}: ${messageOf(cause)}`,
          ),
          ...successes.flatMap(({ result, source }) =>
            result.warnings.map(
              (warning) => `${source.scopeLabel} · ${source.agentLabel}: ${warning}`,
            ),
          ),
        ];
        const sessions = successes
          .flatMap(({ result, source }) =>
            result.sessions.map((row) => sourcedSession(source, row)),
          )
          .sort(compareSessions);
        const result: AggregatedSessionData = {
          sessions,
          warnings,
          partial: failures.length > 0 || successes.some(({ result: value }) => value.partial),
        };
        setData(result);
        setError(null);
        setListUnavailable(false);
        const inspected = currentSessionRef.current;
        if (inspected) {
          const refreshed = result.sessions.find((row) => row.key === inspected.key);
          if (refreshed) {
            currentSessionRef.current = refreshed;
            setCurrentSession(refreshed);
          } else {
            clearInspection();
          }
        }
        if (result.warnings.length > 0) {
          setSelectedKeys(new Set());
          setSelectionMode(false);
        }
        return result;
      } catch (cause) {
        if (!sessionRequestCancelled(cause, controller.signal)) setError(messageOf(cause));
        return null;
      } finally {
        if (listController.current === controller) {
          listController.current = null;
          if (kind === "refresh") setRefreshing(false);
          else setLoadingList(false);
        }
      }
    },
    [api, clearInspection, sources],
  );

  useEffect(() => {
    clearInspection();
    setData(null);
    setError(null);
    setListUnavailable(false);
    setSelectionMode(false);
    setSelectedKeys(new Set());
    setDialogKeys(null);
    setSingleDeleteTarget(null);
    setFocusAfterDelete(undefined);
    void load();
    return () => {
      listController.current?.abort();
      abortPromptStream();
    };
  }, [abortPromptStream, clearInspection, load]);

  useEffect(() => {
    if (!routeSelection || !data || loadingList) return;
    const row = data.sessions.find(
      (candidate) =>
        candidate.source.scopeKey === routeSelection.scopeKey &&
        candidate.source.agent === routeSelection.agent &&
        candidate.id === routeSelection.id,
    );
    if (row) {
      if (currentSessionRef.current?.key !== row.key) void openSession(row, false);
      return;
    }
    setRouteSelection(null);
    clearInspection();
    changePageLocation(
      "sessions",
      sessionLocation(selectedScopes, selectedAgents),
      onLocationChange,
      true,
    );
  }, [
    clearInspection,
    data,
    loadingList,
    onLocationChange,
    openSession,
    routeSelection,
    selectedAgents,
    selectedScopes,
  ]);

  useEffect(() => {
    if (selectionMode || !focusSelectAfterExit.current) return;
    focusSelectAfterExit.current = false;
    const target = selectButton.current;
    if (target && !target.disabled) target.focus();
    else if (refreshButton.current && !refreshButton.current.disabled)
      refreshButton.current.focus();
  }, [selectionMode]);

  useEffect(() => {
    if (focusAfterDelete === undefined || deletion !== null) return;
    const preferred = focusAfterDelete ? deleteButtons.current.get(focusAfterDelete) : null;
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (target && !target.disabled) {
      target.focus();
      setFocusAfterDelete(undefined);
    }
  }, [data, deletion, focusAfterDelete]);

  function toggleSession(key: string) {
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  function toggleAllSessions() {
    const keys = data?.sessions.map((row) => row.key) ?? [];
    const allSelected = keys.length > 0 && keys.every((key) => selectedKeys.has(key));
    setSelectedKeys(allSelected ? new Set() : new Set(keys));
  }

  function cancelSelection() {
    focusSelectAfterExit.current = true;
    setSelectionMode(false);
    setSelectedKeys(new Set());
  }

  function commitScopes(values: ReadonlySet<SessionScopeKey>) {
    const next = new Set(values);
    clearInspection();
    setData(null);
    setRouteSelection(null);
    setSelectedScopes(next);
    changePageLocation("sessions", sessionLocation(next, selectedAgents), onLocationChange);
  }

  function commitAgents(values: ReadonlySet<Agent>) {
    const next = new Set(values);
    clearInspection();
    setData(null);
    setRouteSelection(null);
    setSelectedAgents(next);
    changePageLocation("sessions", sessionLocation(selectedScopes, next), onLocationChange);
  }

  function closeSessionInspection() {
    const focusKey = currentSession?.key ?? null;
    clearInspection();
    setRouteSelection(null);
    changePageLocation(
      "sessions",
      sessionLocation(selectedScopes, selectedAgents),
      onLocationChange,
    );
    window.requestAnimationFrame(() => {
      if (focusKey) sessionRowButtons.current.get(focusKey)?.focus();
    });
  }

  async function requestSessionDeletion(source: SessionSource, ids: string[]) {
    return api.post<{ deleted: number }>("/_aibox/api/sessions/delete", {
      ...scopeBody(source.scope),
      agent: source.agent,
      ids,
      all: false,
      confirmation: "",
    });
  }

  function beginDeletion(next: Exclude<SessionDeletion, null>): boolean {
    if (deletionInFlight.current) return false;
    deletionInFlight.current = true;
    setDeletion(next);
    return true;
  }

  function finishDeletion() {
    deletionInFlight.current = false;
    setDeletion(null);
  }

  async function deleteSession(row: SourcedSession) {
    if (
      operation?.state === "running" ||
      data?.warnings.length ||
      listUnavailable ||
      !data ||
      !beginDeletion({ kind: "record", key: row.key })
    )
      return;
    const originRows = data.sessions;
    const wasCurrent = currentSessionRef.current?.key === row.key;
    if (wasCurrent) abortPromptStream();
    resolveFailure("action");
    try {
      await requestSessionDeletion(row.source, [row.id]);
      setData((current) =>
        current
          ? { ...current, sessions: current.sessions.filter((session) => session.key !== row.key) }
          : current,
      );
      if (wasCurrent) clearInspection();
      await load("refresh");
      setFocusAfterDelete(focusTargetAfterSessionDelete(originRows, row.key));
    } catch (cause) {
      reportFailure("action", "Couldn’t delete Session", cause);
      const refreshed = await load("refresh");
      const survivor = refreshed?.sessions.find((session) => session.key === row.key);
      if (wasCurrent && survivor) void openSession(survivor);
      setFocusAfterDelete(survivor ? row.key : null);
    } finally {
      setSingleDeleteTarget(null);
      finishDeletion();
    }
  }

  async function deleteSelectedSessions() {
    if (
      operation?.state === "running" ||
      !dialogKeys ||
      dialogKeys.length === 0 ||
      !beginDeletion({ kind: "batch" })
    )
      return;
    const keys = dialogKeys;
    const keySet = new Set(keys);
    const selectedRows = data?.sessions.filter((row) => keySet.has(row.key)) ?? [];
    const groups = new Map<string, { source: SessionSource; ids: string[] }>();
    for (const row of selectedRows) {
      const group = groups.get(row.source.key) ?? { source: row.source, ids: [] };
      group.ids.push(row.id);
      groups.set(row.source.key, group);
    }
    const currentKey = currentSessionRef.current?.key;
    const wasCurrent = currentKey ? keySet.has(currentKey) : false;
    if (wasCurrent) clearInspection();
    resolveFailure("action");
    const failures: string[] = [];
    const orderedGroups = [...groups.values()].sort((left, right) =>
      left.source.key.localeCompare(right.source.key),
    );
    for (const { source, ids } of orderedGroups) {
      try {
        await requestSessionDeletion(source, ids);
      } catch (cause) {
        failures.push(`${source.scopeLabel} · ${source.agentLabel}: ${messageOf(cause)}`);
      }
    }
    setDialogKeys(null);
    if (failures.length > 0) {
      reportFailure(
        "action",
        "Couldn’t delete all selected Sessions",
        new Error(failures.join("; ")),
      );
    }
    const refreshed = await load("refresh");
    if (refreshed && refreshed.warnings.length === 0) {
      const remaining = new Set(
        keys.filter((key) => refreshed.sessions.some((row) => row.key === key)),
      );
      setSelectedKeys(remaining);
      setSelectionMode(remaining.size > 0);
      if (wasCurrent && currentKey) {
        const survivor = refreshed.sessions.find((row) => row.key === currentKey);
        if (survivor) void openSession(survivor);
      }
    }
    if (failures.length === 0) setFocusAfterDelete(null);
    finishDeletion();
  }

  const unsafeView = listUnavailable || (data?.warnings.length ?? 0) > 0;
  const sessions = data?.sessions ?? [];
  const allSelected = sessions.length > 0 && sessions.every((row) => selectedKeys.has(row.key));
  const deletionBusy = deletion !== null;
  const mutationBusy = deletionBusy || operation?.state === "running";
  const dialogSessions = dialogKeys
    ? sessions.filter((session) => dialogKeys.includes(session.key))
    : [];
  const dialogSources = [
    ...dialogSessions
      .reduce((groups, session) => {
        const current = groups.get(session.source.key) ?? { source: session.source, count: 0 };
        current.count += 1;
        groups.set(session.source.key, current);
        return groups;
      }, new Map<string, { source: SessionSource; count: number }>())
      .values(),
  ].sort((left, right) => left.source.key.localeCompare(right.source.key));
  const batchBusy = deletion?.kind === "batch";

  function retryPageError() {
    setError(null);
    if (!listUnavailable && currentSessionRef.current) {
      void openSession(currentSessionRef.current, false);
    } else {
      void load("refresh");
    }
  }

  return (
    <div className={`${styles.page} ${styles.catalogPage} ${styles.sessionPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={tenantError ? retryTenants : error ? retryPageError : undefined}
      />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${currentSession ? styles.hasSelection : ""}`}>
        <aside className={`${styles.catalog} ${styles.sessionCatalog}`} aria-label="Sessions">
          <div
            className={`${styles.sessionToolbar} ${selectionMode ? styles.sessionSelectionToolbar : ""}`}
          >
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={styles.sessionCancelSelection}
                  disabled={deletionBusy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={styles.sessionSelectionActions}>
                  <span
                    className={styles.sessionSelectionCount}
                    title={`${selectedKeys.size} selected`}
                  >
                    {selectedKeys.size} selected
                  </span>
                  <button
                    type="button"
                    className={styles.sessionSelectAll}
                    onClick={toggleAllSessions}
                    disabled={sessions.length === 0 || deletionBusy}
                  >
                    {allSelected ? "Clear all" : "Select all"}
                  </button>
                  <button
                    type="button"
                    className={styles.sessionDeleteSelected}
                    aria-label="Delete selected Sessions"
                    disabled={selectedKeys.size === 0 || mutationBusy}
                    onClick={() => setDialogKeys([...selectedKeys])}
                  >
                    <Trash2 size={14} aria-hidden="true" />
                    Delete selected
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className={styles.sessionFilters}>
                  <SessionMultiSelect
                    className={styles.sessionTenantFilter}
                    disabled={loadingTenants || deletionBusy}
                    label="Tenant"
                    onCommit={commitScopes}
                    options={tenantOptions}
                    pluralLabel="tenants"
                    selected={selectedScopes}
                    triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                  />
                  <SessionMultiSelect
                    className={styles.sessionAgentFilter}
                    disabled={deletionBusy}
                    label="Coding Agent"
                    onCommit={commitAgents}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={selectedAgents}
                    triggerIcon={
                      selectedAgents.size === 1 ? (
                        <AgentIcon agent={[...selectedAgents][0] ?? "codex"} size={14} />
                      ) : (
                        <Box size={14} aria-hidden="true" />
                      )
                    }
                  />
                </div>
                <div className={styles.sessionHeaderActions}>
                  <IconButton
                    buttonRef={refreshButton}
                    data-dialog-focus-fallback="true"
                    className={styles.sessionRefresh}
                    label={refreshing ? "Refreshing Sessions" : "Refresh Sessions"}
                    aria-busy={refreshing}
                    disabled={loadingList || refreshing || deletionBusy}
                    onClick={() => void load("refresh")}
                  >
                    <RefreshCw
                      className={refreshing ? "spin" : undefined}
                      size={14}
                      aria-hidden="true"
                    />
                  </IconButton>
                  <button
                    ref={selectButton}
                    type="button"
                    className={styles.sessionSelect}
                    aria-label="Select Sessions"
                    title="Select Sessions"
                    disabled={
                      sessions.length === 0 ||
                      unsafeView ||
                      loadingList ||
                      refreshing ||
                      deletionBusy
                    }
                    onClick={() => setSelectionMode(true)}
                  >
                    <ListChecks size={14} aria-hidden="true" />
                    Select
                  </button>
                </div>
              </>
            )}
          </div>
          <div className={styles.sessionWarnings}>
            {data?.warnings.map((warning) => (
              <div className={styles.inlineWarning} key={warning}>
                <AlertTriangle size={15} aria-hidden="true" />
                <span>{warning}</span>
              </div>
            ))}
          </div>
          <div className={`${styles.catalogList} ${styles.sessionList}`} aria-busy={loadingList}>
            {!data && loadingList && <Loading />}
            {sessions.map((row) => {
              const selectedForDeletion = selectedKeys.has(row.key);
              const deleting = deletion?.kind === "record" && deletion.key === row.key;
              const title = row.title || "Untitled Session";
              const sourceDescription = `${row.source.scopeLabel} · ${row.source.agentLabel}`;
              return (
                <div
                  key={row.key}
                  className={[
                    styles.sessionRow,
                    currentSession?.key === row.key ? styles.currentSessionRow : "",
                    selectionMode ? styles.sessionSelectionRow : "",
                    selectedForDeletion ? styles.sessionRowSelected : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                >
                  <button
                    ref={(element) => {
                      if (element) sessionRowButtons.current.set(row.key, element);
                      else sessionRowButtons.current.delete(row.key);
                    }}
                    type="button"
                    className={styles.sessionRowMain}
                    aria-label={
                      selectionMode
                        ? `${selectedForDeletion ? "Deselect" : "Select"} ${title}, ${sourceDescription}`
                        : `${title}, ${sourceDescription}`
                    }
                    aria-pressed={selectionMode ? selectedForDeletion : undefined}
                    disabled={deletionBusy || loadingList}
                    onClick={() => (selectionMode ? toggleSession(row.key) : void openSession(row))}
                  >
                    <SessionIcon size={16} data-icon="session-record" aria-hidden="true" />
                    <span>
                      <strong title={title}>{title}</strong>
                      <small className={styles.sessionRowMetadata}>
                        <span>
                          {sessionListScopeLabel(row.source.scopeKey)} · {row.source.agentLabel}
                        </span>
                        <time dateTime={row.start_ts}>{formatTimestamp(row.start_ts)}</time>
                      </small>
                    </span>
                    {row.warnings.length > 0 && (
                      <span
                        className={styles.sessionRowWarning}
                        role="img"
                        aria-label={`Session has ${row.warnings.length} Transcript warning${row.warnings.length === 1 ? "" : "s"}`}
                        title={row.warnings.join("\n")}
                      >
                        <AlertTriangle size={14} aria-hidden="true" />
                      </span>
                    )}
                    {selectionMode && (
                      <span className={styles.sessionSelectionIndicator} aria-hidden="true">
                        {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                      </span>
                    )}
                  </button>
                  {!selectionMode && (
                    <button
                      ref={(element) => {
                        if (element) deleteButtons.current.set(row.key, element);
                        else deleteButtons.current.delete(row.key);
                      }}
                      type="button"
                      className={styles.sessionDelete}
                      title={`Delete Session ${row.display_id} from ${sourceDescription}`}
                      aria-label={
                        deleting
                          ? `Deleting Session ${row.display_id} from ${sourceDescription}`
                          : `Delete Session ${row.display_id} from ${sourceDescription}`
                      }
                      aria-busy={deleting}
                      disabled={unsafeView || mutationBusy || loadingList}
                      onClick={() => setSingleDeleteTarget(row)}
                    >
                      {deleting ? (
                        <LoaderCircle className="spin" size={15} aria-hidden="true" />
                      ) : (
                        <Trash2 size={15} aria-hidden="true" />
                      )}
                    </button>
                  )}
                </div>
              );
            })}
            {data?.sessions.length === 0 && !loadingList && (
              <EmptyState
                variant="list"
                icon={<SessionIcon size={22} data-icon="session-list-empty" aria-hidden="true" />}
                title="No Sessions found"
                description="No Sessions were found for the selected Tenants and Coding Agents."
              />
            )}
          </div>
        </aside>
        <section className={styles.detailPane}>
          {currentSession ? (
            <>
              <div className={styles.detailHeader}>
                <IconButton label="Back to Sessions" onClick={closeSessionInspection}>
                  <ChevronLeft size={17} />
                </IconButton>
                <div>
                  <h2 ref={detailHeadingRef} tabIndex={-1}>
                    {currentSession.title || "Untitled Session"}
                  </h2>
                  <span className={styles.sessionDetailSource}>
                    {currentSession.source.scopeLabel} · {currentSession.source.agentLabel} ·{" "}
                    <code>{currentSession.id}</code>
                  </span>
                </div>
              </div>
              {[...currentSession.warnings, ...promptWarnings].map((warning) => (
                <div className={styles.inlineWarning} key={warning}>
                  {warning}
                </div>
              ))}
              <div className={styles.promptList}>
                {prompts.map((prompt, index) => (
                  <article key={`${index}:${prompt.timestamp}`}>
                    <header>
                      <span>Prompt {index + 1}</span>
                      <time>{prompt.timestamp}</time>
                    </header>
                    <pre>{prompt.text}</pre>
                  </article>
                ))}
                {loadingPrompts && <Loading />}
                {!loadingPrompts && prompts.length === 0 && (
                  <EmptyState
                    className={styles.promptEmptyState}
                    variant="detail"
                    icon={<SessionIcon size={26} aria-hidden="true" />}
                    title="No typed prompts"
                    description="This Session's Transcript contains no supported typed user prompts."
                  />
                )}
              </div>
            </>
          ) : (
            <EmptyState
              variant="detail"
              icon={<SessionIcon size={26} data-icon="session-empty" aria-hidden="true" />}
              title="Select a Session"
              description="Choose a Session to inspect its prompts and Transcript warnings."
            />
          )}
        </section>
      </div>
      <NotificationCenter
        notifications={notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogKeys !== null || singleDeleteTarget !== null}
        onAction={() => undefined}
        onDismiss={dismissNotification}
      />
      {singleDeleteTarget && (
        <ConfirmDialog
          title={`Delete Session ${singleDeleteTarget.display_id}?`}
          message={`This permanently deletes its Transcript from ${singleDeleteTarget.source.scopeLabel} · ${singleDeleteTarget.source.agentLabel}.`}
          confirmLabel="Delete permanently"
          busy={deletion?.kind === "record" || operation?.state === "running"}
          onCancel={() => {
            if (deletion?.kind !== "record") setSingleDeleteTarget(null);
          }}
          onConfirm={() => void deleteSession(singleDeleteTarget)}
        />
      )}
      {dialogKeys && (
        <ConfirmDialog
          title={`Delete ${dialogKeys.length} selected Session${dialogKeys.length === 1 ? "" : "s"}?`}
          message={`This permanently deletes the Transcripts for the selected Sessions. Sources: ${dialogSources
            .map(({ count, source }) => `${source.scopeLabel} · ${source.agentLabel} (${count})`)
            .join("; ")}.`}
          confirmLabel="Delete permanently"
          busy={batchBusy || operation?.state === "running"}
          onCancel={() => {
            if (!batchBusy) setDialogKeys(null);
          }}
          onConfirm={() => void deleteSelectedSessions()}
        />
      )}
    </div>
  );
}

export function OperationPanel({
  api,
  operation,
  connection = "connected",
  onOperation,
  onDismiss,
  onExpandedChange,
}: {
  api: ControlApi;
  operation: Operation;
  connection?: "connecting" | "connected" | "reconnecting";
  onOperation: (operation: Operation) => void;
  onDismiss: () => void;
  onExpandedChange?: (expanded: boolean) => void;
}) {
  const [expanded, setExpanded] = useState(operation.state !== "succeeded");
  const [cancelRequested, setCancelRequested] = useState(false);
  const [panelError, setPanelError] = useState<string | null>(null);

  useEffect(() => {
    onExpandedChange?.(expanded);
  }, [expanded, onExpandedChange]);

  useEffect(() => {
    setCancelRequested(false);
    setPanelError(null);
    setExpanded(operation.state !== "succeeded");
  }, [operation.id, operation.state]);

  useEffect(() => {
    if (operation.state === "failed" || operation.state === "cancelled") setExpanded(true);
    else if (operation.state === "succeeded") setExpanded(false);
  }, [operation.state]);

  async function cancel() {
    if (cancelRequested) return;
    setCancelRequested(true);
    setPanelError(null);
    try {
      await api.post(`/_aibox/api/operations/${encodeURIComponent(operation.id)}/cancel`);
    } catch (cause) {
      setCancelRequested(false);
      setPanelError(messageOf(cause));
    }
  }
  return (
    <section
      className={`${styles.operationPanel} ${expanded ? styles.operationPanelExpanded : ""}`}
      aria-label="Management Operation"
    >
      <header>
        <div>
          {operation.state === "running" ? (
            <LoaderCircle className="spin" size={16} />
          ) : operation.state === "succeeded" ? (
            <Check size={16} />
          ) : (
            <CircleStop size={16} />
          )}
          <strong>{operation.kind}</strong>
        </div>
        <span aria-live="polite">
          {cancelRequested && operation.state === "running"
            ? "Cancellation requested"
            : operation.state}
        </span>
        {operation.state === "running" && (
          <IconButton
            label={cancelRequested ? "Cancellation requested" : "Cancel operation"}
            disabled={cancelRequested}
            onClick={() => void cancel()}
          >
            <CircleStop size={16} />
          </IconButton>
        )}
        <IconButton
          label="Refresh operation"
          onClick={() =>
            void api
              .get<{ operation: Operation | null }>("/_aibox/api/operations/current")
              .then((value) => value.operation && onOperation(value.operation))
          }
        >
          <RefreshCw size={15} />
        </IconButton>
        <IconButton
          label={expanded ? "Collapse operation" : "Expand operation"}
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronDown size={15} /> : <ChevronUp size={15} />}
        </IconButton>
        {operation.state !== "running" && (
          <IconButton label="Dismiss operation" onClick={onDismiss}>
            <X size={15} />
          </IconButton>
        )}
      </header>
      {expanded && (
        <>
          {operation.first_sequence > 0 && (
            <div className={styles.operationGap} role="status">
              Earlier log output was truncated; showing entries from #{operation.first_sequence}.
            </div>
          )}
          <pre>
            {operation.logs.map((entry) => entry.message).join("\n") ||
              operation.result ||
              "Connected · waiting for output"}
          </pre>
          {panelError && <div className={styles.operationError}>{panelError}</div>}
          <footer>
            <span>
              {operation.state !== "running"
                ? "Terminal state"
                : connection === "connected"
                  ? "Live updates connected"
                  : connection === "reconnecting"
                    ? "Reconnecting to live updates"
                    : "Connecting to live updates"}
            </span>
            {operation.result && <strong>{operation.result}</strong>}
          </footer>
        </>
      )}
    </section>
  );
}
