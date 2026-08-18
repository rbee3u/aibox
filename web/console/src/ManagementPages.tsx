/* eslint-disable react-hooks/set-state-in-effect */

import {
  AlertTriangle,
  Box,
  Check,
  ChevronDown,
  ChevronLeft,
  CircleStop,
  Download,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ControlApi, decodeBase64, encodeBase64, scopeBody, scopeQuery } from "./controlApi";
import type {
  Agent,
  ApplicationStatus,
  ComponentRow,
  ConfigCatalogEntry,
  ConfigFileData,
  ConfigListData,
  Operation,
  Prompt,
  PropagationPreview,
  PropagationReport,
  Scope,
  SessionListData,
  SessionRow,
  TenantRow,
} from "./controlApi";
import { ConfirmDialog as DestructiveConfirmDialog } from "./components/ConfirmDialog";
import { IssueIndicator, type IssueTone } from "./components/IssueIndicator";
import { NotificationCenter } from "./components/NotificationCenter";
import { useFailureNotifications } from "./useFailureNotifications";
import { AgentIcon } from "./icons";
import { formatTimestamp } from "./utils";
import type { ModuleId } from "./moduleIcons";
import { resourceIcons } from "./resourceIcons";
import styles from "./ManagementPages.module.css";

const ComponentIcon = resourceIcons.component;
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
const SessionIcon = resourceIcons.session;

interface PageProps {
  api: ControlApi;
  locationVersion?: number;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange?: (module: ModuleId, query: URLSearchParams, replace?: boolean) => void;
  onOperation?: (operation: Operation) => void;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function PageError({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <div className={styles.errorBanner} role="alert">
      <AlertTriangle size={16} aria-hidden="true" />
      <span>{error}</span>
    </div>
  );
}

function Loading() {
  return (
    <div className={styles.loading}>
      <LoaderCircle size={22} aria-label="Loading" />
    </div>
  );
}

function IconButton({
  label,
  children,
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button
      className={`${styles.iconButton} ${className ?? ""}`}
      type="button"
      title={label}
      aria-label={label}
      {...props}
    >
      {children}
    </button>
  );
}

function ConfirmDialog({
  title,
  description,
  confirmation,
  confirmLabel,
  busy,
  onCancel,
  onConfirm,
}: {
  title: string;
  description?: ReactNode;
  confirmation?: string;
  confirmLabel: string;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const enabled = !confirmation || typed === confirmation;
  return (
    <div className={styles.dialogBackdrop} role="presentation" onMouseDown={onCancel}>
      <section
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2 id="confirm-title">{title}</h2>
        {description}
        {confirmation && (
          <label>
            Type <code>{confirmation}</code> to confirm
            <input autoFocus value={typed} onChange={(event) => setTyped(event.target.value)} />
          </label>
        )}
        <div className={styles.dialogActions}>
          <button type="button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button
            className={styles.dangerButton}
            type="button"
            onClick={onConfirm}
            disabled={!enabled || busy}
          >
            {busy && <LoaderCircle size={14} aria-hidden="true" />} {confirmLabel}
          </button>
        </div>
      </section>
    </div>
  );
}

function tenantScope(row: TenantRow): Scope {
  return row.kind === "host" ? { scope: "host" } : { scope: "managed", tenant: row.name! };
}

type TenantKey = "host" | `managed:${string}`;
type TenantDeleteTarget = { names: string[] };

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

export function TenantPage({ api, locationVersion = 0, onLocationChange, onOperation }: PageProps) {
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
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<TenantKey>>(new Set());
  const [deleteTarget, setDeleteTarget] = useState<TenantDeleteTarget | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(initialKey !== null);
  const preserveComponentError = useRef(false);
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
  const selectableKeys = managedTenants.map((row) => tenantKeyOf(row));
  const allSelectable =
    selectableKeys.length > 0 && selectableKeys.every((key) => selectedKeys.has(key));
  const selectedCount = selectedKeys.size;
  const createNameValid = CONFIG_NAME_PATTERN.test(newName);

  useEffect(() => {
    if (observedLocationVersion.current === locationVersion) return;
    observedLocationVersion.current = locationVersion;
    const query = pageSearch();
    const key = tenantKeyFromParam(query.get("scope"));
    setSelectedKey(key);
    setSelectedComponent(key ? query.get("component") : null);
    setDetailOpen(key !== null);
  }, [locationVersion]);

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
        return new Set([...current].filter((key) => available.has(key) && key !== "host"));
      });
      setError(null);
      return rows;
    } catch (cause) {
      setError(messageOf(cause));
      return null;
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

  async function refreshTenants() {
    setRefreshing(true);
    try {
      await loadTenants();
    } finally {
      setRefreshing(false);
    }
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
    if (key === "host") return;
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
      <PageError error={error} />
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
                    disabled={selectedCount === 0 || busy}
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
                <button
                  type="button"
                  className={styles.sessionRefresh}
                  aria-label={refreshing ? "Refreshing Tenants" : "Refresh Tenants"}
                  aria-busy={refreshing}
                  disabled={refreshing || busy}
                  onClick={() => void refreshTenants()}
                >
                  <RefreshCw className={refreshing ? styles.spinning : undefined} size={14} />
                  Refresh
                </button>
                <button
                  type="button"
                  className={styles.sessionSelect}
                  aria-label="Select Tenants"
                  disabled={selectableKeys.length === 0 || refreshing || busy}
                  onClick={() => setSelectionMode(true)}
                >
                  <ListChecks size={14} /> Select
                </button>
              </div>
            )}
          </div>
          <div className={styles.configList} aria-busy={refreshing}>
            <div className={styles.configRowGroup}>
              {hostTenant && (
                <div
                  className={`${styles.configRow} ${selectedKey === "host" ? styles.configRowInspected : ""} ${selectionMode ? `${styles.configRowSelection} ${styles.configRowProtected}` : ""}`}
                >
                  <button
                    type="button"
                    className={styles.configRowMain}
                    aria-label={selectionMode ? "Host Tenant cannot be selected" : "Host Tenant"}
                    aria-pressed={!selectionMode && selectedKey === "host"}
                    disabled={busy || refreshing || selectionMode}
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
                        {abbreviateTenantHome(hostTenant.home, hostTenant.home)}
                      </small>
                    </span>
                    {selectionMode && <span className={styles.configProtected}>Protected</span>}
                  </button>
                </div>
              )}
              <div className={styles.catalogDivider}>
                <span>Managed Tenants</span>
                <IconButton
                  className={styles.configAddButton}
                  label="Create Tenant"
                  disabled={busy || refreshing || selectionMode}
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
                const selectedForInspection = key === selectedKey;
                const selectedForDeletion = selectedKeys.has(key);
                return (
                  <div
                    key={key}
                    className={`${styles.configRow} ${selectedForInspection ? styles.configRowInspected : ""} ${selectedForDeletion ? styles.configRowSelected : ""} ${selectionMode ? styles.configRowSelection : ""}`}
                  >
                    <button
                      type="button"
                      className={styles.configRowMain}
                      aria-label={
                        selectionMode
                          ? `${selectedForDeletion ? "Deselect" : "Select"} ${row.display_name}`
                          : row.display_name
                      }
                      aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                      disabled={busy || refreshing}
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
                          {abbreviateTenantHome(row.home, hostTenant?.home ?? null)}
                        </small>
                      </span>
                      {selectionMode && (
                        <span className={styles.sessionSelectionIndicator} aria-hidden="true">
                          {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                        </span>
                      )}
                    </button>
                    {!selectionMode && (
                      <div className={styles.configRowActions}>
                        <IconButton
                          className={`${styles.configRowAction} ${styles.configDeleteAction}`}
                          label={`Delete Tenant ${row.display_name}`}
                          disabled={busy}
                          onClick={() => requestTenantDelete([row.name])}
                        >
                          <Trash2 size={15} />
                        </IconButton>
                      </div>
                    )}
                  </div>
                );
              })}
              {managedTenants.length === 0 && (
                <div className={styles.configListEmpty}>No Managed Tenants found.</div>
              )}
            </div>
          </div>
        </aside>
        <section className={styles.detailPane}>
          {selected ? (
            <>
              <div className={styles.detailHeader}>
                <IconButton
                  label="Back to Tenants"
                  onClick={() => {
                    setSelectedKey(null);
                    setSelectedComponent(null);
                    setDetailOpen(false);
                    changePageLocation("tenants", new URLSearchParams(), onLocationChange);
                  }}
                >
                  <ChevronLeft size={17} />
                </IconButton>
                <div>
                  <h2>{selected.display_name}</h2>
                  <code>{selected.home}</code>
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
                  const installed = row.status && row.status !== "not-installed";
                  return (
                    <div
                      className={`${styles.componentRow} ${selectedComponent === row.kind ? styles.componentRowSelected : ""}`}
                      key={row.kind}
                      aria-current={selectedComponent === row.kind ? "true" : undefined}
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
                      <div>
                        <strong>{row.kind}</strong>
                        <small>{row.error ?? row.status ?? "Unavailable"}</small>
                      </div>
                      {row.supports_version && !installed && (
                        <input
                          aria-label={`${row.kind} version`}
                          placeholder="stable"
                          value={versions[row.kind] ?? ""}
                          onChange={(event) =>
                            setVersions((value) => ({ ...value, [row.kind]: event.target.value }))
                          }
                        />
                      )}
                      <span className={installed ? styles.goodStatus : styles.neutralStatus}>
                        {row.version ?? row.status}
                      </span>
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void mutateComponent(row, !installed)}
                      >
                        {installed ? <Trash2 size={14} /> : <Download size={14} />}
                        {installed ? "Remove" : "Install"}
                      </button>
                    </div>
                  );
                })}
              </div>
            </>
          ) : (
            <div className={styles.emptyPane}>
              <Box size={24} />
              <span>Select a Tenant</span>
            </div>
          )}
        </section>
      </div>
      {createOpen && (
        <div className={styles.dialogBackdrop} onMouseDown={() => !busy && setCreateOpen(false)}>
          <form
            className={styles.dialog}
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-tenant-title"
            onMouseDown={(event) => event.stopPropagation()}
            onSubmit={(event) => {
              event.preventDefault();
              if (createNameValid && !busy) void createTenant();
            }}
          >
            <h2 id="create-tenant-title">Create Tenant</h2>
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
              />
            </label>
            {createError && <div className={styles.inlineWarning}>{createError}</div>}
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setCreateOpen(false)} disabled={busy}>
                Cancel
              </button>
              <button
                className={styles.primaryButton}
                type="submit"
                disabled={!createNameValid || busy}
              >
                <Plus size={14} /> Create
              </button>
            </div>
          </form>
        </div>
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
          confirmLabel="Delete Tenant"
          busy={busy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenants()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <div className={styles.dialogBackdrop}>
          <section
            className={`${styles.dialog} ${styles.wideDialog}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="delete-tenants-title"
          >
            <h2 id="delete-tenants-title">Delete selected Managed Tenants?</h2>
            <p className={styles.dialogDescription}>
              This permanently deletes each Tenant Home, its Sessions and Components state, and its
              Named Configs.
            </p>
            <div className={styles.planList}>
              {deleteTarget.names.map((name) => (
                <code key={name}>{name}</code>
              ))}
            </div>
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setDeleteTarget(null)} disabled={busy}>
                Cancel
              </button>
              <button
                className={styles.dangerButton}
                type="button"
                onClick={() => void deleteTenants()}
                disabled={busy}
              >
                <Trash2 size={14} /> Delete selected
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

function useTenants(api: ControlApi) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  useEffect(() => {
    void api.get<TenantRow[]>("/_aibox/api/tenants").then(setTenants);
  }, [api]);
  return tenants;
}

type ConfigSelection = { current: true; config?: never } | { current: false; config: string };
type ConfigScopeKey = "host" | `managed:${string}`;
type ConfigDeleteTarget = { names: string[] };
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

export function ConfigPage({
  api,
  locationVersion = 0,
  onDirtyChange,
  onLocationChange,
}: PageProps) {
  const [initialRoute] = useState(readConfigRoute);
  const observedLocationVersion = useRef(locationVersion);
  const tenants = useTenants(api);
  const [scope, setScope] = useState<Scope>(initialRoute.scope);
  const [agent, setAgent] = useState<Agent>(initialRoute.agent);
  const [catalog, setCatalog] = useState<ConfigListData | null>(null);
  const [selection, setSelection] = useState<ConfigSelection>(initialRoute.selection);
  const [file, setFile] = useState<string | null>(initialRoute.file);
  const [snapshot, setSnapshot] = useState<ConfigFileData | null>(null);
  const [editor, setEditor] = useState("");
  const [editorMode, setEditorMode] = useState<"text" | "base64">("text");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedNames, setSelectedNames] = useState<Set<string>>(new Set());
  const [deleteTarget, setDeleteTarget] = useState<ConfigDeleteTarget | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<ConfigPendingAction | null>(null);
  const [detailOpen, setDetailOpen] = useState(initialRoute.detailOpen);
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);
  const catalogController = useRef<AbortController | null>(null);
  const fileLoadGeneration = useRef(0);

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

  const tenantOptions = useMemo<SessionFilterOption<ConfigScopeKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter((tenant): tenant is TenantRow & { kind: "managed"; name: string } =>
        Boolean(tenant.kind === "managed" && tenant.name),
      )
      .sort((left, right) => left.name.localeCompare(right.name));
    if (!managed.some((tenant) => tenant.name === "default")) {
      managed.push({
        kind: "managed",
        name: "default",
        display_name: "default",
        home: "",
        exists: false,
      });
      managed.sort((left, right) => left.name.localeCompare(right.name));
    }
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
        label: tenant.exists ? tenant.display_name : `${tenant.display_name} (not created)`,
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
              [...current].filter(
                (name) =>
                  data.configs.some((entry) => entry.name === name) &&
                  data.application.last_application?.applied !== name,
              ),
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
  const selectableNames =
    catalog?.configs.filter((entry) => entry.name !== appliedName).map((entry) => entry.name) ?? [];
  const allSelectable =
    selectableNames.length > 0 && selectableNames.every((name) => selectedNames.has(name));

  const editorBytes = useMemo(() => {
    if (!snapshot) return null;
    try {
      return editorMode === "text"
        ? new TextEncoder().encode(editor)
        : decodeBase64(editor.replace(/\s/g, ""));
    } catch {
      return null;
    }
  }, [editor, editorMode, snapshot]);
  const editorDirty =
    snapshot !== null &&
    (editorBytes === null || encodeBase64(editorBytes) !== snapshot.content_base64);

  useEffect(() => onDirtyChange?.(editorDirty), [editorDirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);

  function setEditorFromSnapshot(value: ConfigFileData) {
    const bytes = decodeBase64(value.content_base64);
    try {
      setEditor(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      setEditorMode("text");
    } catch {
      setEditor(value.content_base64);
      setEditorMode("base64");
    }
  }

  useEffect(() => {
    if (!catalog || !file) {
      setSnapshot(null);
      setEditor("");
      return;
    }
    const generation = ++fileLoadGeneration.current;
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
        setEditorFromSnapshot(value);
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
  }, [agent, api, catalog, file, scope, selection]);

  function switchEditorMode(next: "text" | "base64") {
    if (next === editorMode) return;
    try {
      if (next === "base64") {
        setEditor(encodeBase64(new TextEncoder().encode(editor)));
      } else {
        setEditor(
          new TextDecoder("utf-8", { fatal: true }).decode(decodeBase64(editor.replace(/\s/g, ""))),
        );
      }
      setEditorMode(next);
      setError(null);
    } catch (cause) {
      setError(`Cannot convert editor content: ${messageOf(cause)}`);
    }
  }

  async function saveFile(refreshCatalog: boolean): Promise<boolean> {
    if (!snapshot || !file || editorBytes === null) return false;
    setBusy(true);
    try {
      const value = await api.post<ConfigFileData>("/_aibox/api/configs/save", {
        ...scopeBody(scope),
        agent,
        current: selection.current,
        config: selection.current ? null : selection.config,
        file,
        revision: snapshot.revision,
        content_base64: encodeBase64(editorBytes),
      });
      setEditorFromSnapshot(value);
      setSnapshot(value);
      setError(null);
      if (refreshCatalog) await loadCatalog("background");
      return true;
    } catch (cause) {
      setError(messageOf(cause));
      return false;
    } finally {
      setBusy(false);
    }
  }

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
    if (!name) return;
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
    setBusy(true);
    try {
      await api.post("/_aibox/api/configs/apply", { ...scopeBody(scope), agent, config: name });
      await loadCatalog("background");
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function deleteConfigs() {
    if (!deleteTarget || deleteTarget.names.length === 0) return;
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
        const remaining = requestedNames.filter(
          (name) =>
            refreshed.configs.some((entry) => entry.name === name) &&
            refreshed.application.last_application?.applied !== name,
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
    if (!preview) return;
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

  return (
    <div className={`${styles.page} ${styles.configPage}`}>
      <PageError error={error} />
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
                    disabled={selectedCount === 0 || busy}
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
                    label="Agent"
                    onCommit={selectAgent}
                    options={agentOptions}
                    pluralLabel="agents"
                    selected={new Set([agent])}
                    triggerIcon={<AgentIcon agent={agent} size={14} />}
                    allowMultiple={false}
                  />
                </div>
                <div className={styles.sessionHeaderActions}>
                  <button
                    type="button"
                    className={styles.sessionRefresh}
                    aria-label={refreshing ? "Refreshing Configs" : "Refresh Configs"}
                    aria-busy={refreshing}
                    disabled={loadingCatalog || refreshing || busy}
                    onClick={() =>
                      requestEditorAction(async () => {
                        await loadCatalog("refresh");
                      })
                    }
                  >
                    <RefreshCw className={refreshing ? styles.spinning : undefined} size={14} />
                    Refresh
                  </button>
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
          <div className={styles.configWarnings}>
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
            {loadingCatalog && !catalog && <Loading />}
            <div className={styles.configRowGroup}>
              <div
                className={`${styles.configRow} ${selection.current ? styles.configRowInspected : ""} ${selectionMode ? `${styles.configRowSelection} ${styles.configRowProtected}` : ""}`}
              >
                <button
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
                    <strong>Current</strong>
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
                      disabled={busy}
                      onClick={() => void previewPropagation()}
                    >
                      Propagate
                    </button>
                  )}
              </div>
              <div className={styles.catalogDivider}>
                <span>Named Configs</span>
                <IconButton
                  className={styles.configAddButton}
                  label="Create Named Config"
                  disabled={busy || loadingCatalog || selectionMode}
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
                    className={`${styles.configRow} ${selectedForInspection ? styles.configRowInspected : ""} ${selectedForDeletion ? styles.configRowSelected : ""} ${selectionMode ? styles.configRowSelection : ""} ${selectionMode && applied ? styles.configRowProtected : ""}`}
                  >
                    <button
                      type="button"
                      className={styles.configRowMain}
                      aria-label={
                        selectionMode
                          ? applied
                            ? `${entry.name} is Applied and cannot be selected`
                            : `${selectedForDeletion ? "Deselect" : "Select"} ${entry.name}`
                          : entry.name
                      }
                      aria-describedby={issueDescriptionId}
                      aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                      disabled={busy || loadingCatalog || (selectionMode && applied)}
                      onClick={() =>
                        selectionMode
                          ? applied
                            ? undefined
                            : toggleConfig(entry.name)
                          : void openConfig(entry.name)
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
                      {selectionMode && !applied && (
                        <span className={styles.sessionSelectionIndicator} aria-hidden="true">
                          {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                        </span>
                      )}
                      {selectionMode && applied && (
                        <span className={styles.configProtected}>Protected</span>
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
                                : `Apply Named Config ${entry.name}`
                            }
                            aria-label={`Apply Named Config ${entry.name}`}
                            disabled={busy || (applied && catalog.application.drift === "clean")}
                            onClick={() => requestEditorAction(() => applyConfig(entry.name))}
                          >
                            Apply
                          </button>
                        )}
                        {entry.state === "incomplete" && (
                          <button
                            type="button"
                            className={styles.configRowPrimaryAction}
                            title={`Repair Named Config ${entry.name}`}
                            aria-label={`Repair Named Config ${entry.name}`}
                            disabled={busy}
                            onClick={() => requestEditorAction(() => createConfig(entry.name))}
                          >
                            Repair
                          </button>
                        )}
                        <IconButton
                          className={`${styles.configRowAction} ${styles.configDeleteAction}`}
                          label={`Delete Named Config ${entry.name}`}
                          disabled={busy}
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
                <div className={styles.configListEmpty}>No Named Configs found.</div>
              )}
            </div>
          </div>
        </aside>
        <section className={styles.configEditor}>
          {catalog ? (
            <>
              <div className={styles.configEditorHeader}>
                <IconButton
                  label="Back to Configs"
                  onClick={() =>
                    requestEditorAction(() => {
                      setDetailOpen(false);
                      changePageLocation(
                        "configs",
                        configLocation(scope, agent, null),
                        onLocationChange,
                      );
                    })
                  }
                >
                  <ChevronLeft size={17} />
                </IconButton>
                {catalog.files.length > 1 ? (
                  <div className={styles.fileTabs} role="tablist" aria-label="Config files">
                    {catalog.files.map((name) => (
                      <button
                        type="button"
                        role="tab"
                        aria-selected={file === name}
                        key={name}
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
                  <h2>{file ?? "Configuration"}</h2>
                )}
              </div>
              {loadingFile ? (
                <Loading />
              ) : snapshot ? (
                <>
                  <div className={styles.editorTools}>
                    <span>{snapshot.exists ? "Existing file" : "New file"}</span>
                    <div className={styles.segmented} aria-label="Editor encoding">
                      <button
                        type="button"
                        aria-pressed={editorMode === "text"}
                        onClick={() => switchEditorMode("text")}
                      >
                        UTF-8
                      </button>
                      <button
                        type="button"
                        aria-pressed={editorMode === "base64"}
                        onClick={() => switchEditorMode("base64")}
                      >
                        Base64
                      </button>
                    </div>
                    <button
                      className={styles.primaryButton}
                      type="button"
                      disabled={busy || !editorDirty || editorBytes === null}
                      onClick={() => void saveFile(true)}
                    >
                      <Save size={14} /> Save
                    </button>
                  </div>
                  <textarea
                    className={styles.codeEditor}
                    aria-label={`${file} content`}
                    spellCheck={false}
                    value={editor}
                    onChange={(event) => setEditor(event.target.value)}
                  />
                </>
              ) : (
                <div className={styles.emptyPane}>
                  <NamedConfigIcon size={22} />
                  <span>Unable to load {file ?? "configuration"}.</span>
                </div>
              )}
            </>
          ) : (
            <Loading />
          )}
        </section>
      </div>
      {pendingAction && (
        <div className={styles.dialogBackdrop}>
          <section
            className={styles.dialog}
            role="dialog"
            aria-modal="true"
            aria-labelledby="config-unsaved-title"
          >
            <h2 id="config-unsaved-title">Unsaved changes</h2>
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
                disabled={busy || editorBytes === null}
              >
                Save and continue
              </button>
            </div>
          </section>
        </div>
      )}
      {createOpen && (
        <div className={styles.dialogBackdrop} onMouseDown={() => !busy && setCreateOpen(false)}>
          <form
            className={styles.dialog}
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-config-title"
            onMouseDown={(event) => event.stopPropagation()}
            onSubmit={(event) => {
              event.preventDefault();
              if (createNameValid && !busy) void createConfig(newName);
            }}
          >
            <h2 id="create-config-title">Create Named Config</h2>
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
              />
            </label>
            {createError && <div className={styles.inlineWarning}>{createError}</div>}
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setCreateOpen(false)} disabled={busy}>
                Cancel
              </button>
              <button
                className={styles.primaryButton}
                type="submit"
                disabled={!createNameValid || busy}
              >
                <Plus size={14} /> Create
              </button>
            </div>
          </form>
        </div>
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
          busy={busy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <div className={styles.dialogBackdrop}>
          <section
            className={`${styles.dialog} ${styles.wideDialog}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="delete-configs-title"
          >
            <h2 id="delete-configs-title">Delete selected Named Configs?</h2>
            <p className={styles.dialogDescription}>
              This deletes only the selected Named Configs. Current Config files are not changed.
            </p>
            <div className={styles.planList}>
              {deleteTarget.names.map((name) => (
                <code key={name}>{name}</code>
              ))}
            </div>
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setDeleteTarget(null)} disabled={busy}>
                Cancel
              </button>
              <button
                className={styles.dangerButton}
                type="button"
                onClick={() => void deleteConfigs()}
                disabled={busy}
              >
                <Trash2 size={14} /> Delete selected
              </button>
            </div>
          </section>
        </div>
      )}
      {(preview || report) && (
        <div className={styles.dialogBackdrop}>
          <section
            className={`${styles.dialog} ${styles.wideDialog}`}
            role="dialog"
            aria-modal="true"
          >
            <h2>{preview ? "Credential Propagation preview" : "Credential Propagation result"}</h2>
            <div className={styles.planList}>
              {(preview?.preview.entries ?? report?.entries ?? []).map((entry) => (
                <div key={entry.label}>
                  <code>{entry.label}</code>
                  <span>
                    {entry.outcome.status === "updated" && preview
                      ? "update"
                      : entry.outcome.status}
                  </span>
                </div>
              ))}
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
                  disabled={busy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  Apply {preview.preview.updates} update{preview.preview.updates === 1 ? "" : "s"}
                </button>
              )}
            </div>
          </section>
        </div>
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
                    title={option.label}
                    onClick={() => commitOnly(option.value)}
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

export function SessionPage({ api, locationVersion = 0, onLocationChange }: PageProps) {
  const [initialRoute] = useState(readSessionRoute);
  const observedLocationVersion = useRef(locationVersion);
  const tenants = useTenants(api);
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
  const [deletion, setDeletion] = useState<SessionDeletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [listUnavailable, setListUnavailable] = useState(false);
  const listController = useRef<AbortController | null>(null);
  const streamController = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SourcedSession | null>(null);
  const deletionInFlight = useRef(false);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const selectButton = useRef<HTMLButtonElement>(null);
  const focusSelectAfterExit = useRef(false);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();

  const tenantOptions = useMemo<SessionFilterOption<SessionScopeKey>[]>(() => {
    const host = tenants.find((tenant) => tenant.kind === "host");
    const managed = tenants
      .filter((tenant): tenant is TenantRow & { kind: "managed"; name: string } =>
        Boolean(tenant.kind === "managed" && tenant.name),
      )
      .sort((left, right) => left.name.localeCompare(right.name));
    if (!managed.some((tenant) => tenant.name === "default")) {
      managed.push({
        kind: "managed",
        name: "default",
        display_name: "default",
        home: "",
        exists: false,
      });
      managed.sort((left, right) => left.name.localeCompare(right.name));
    }
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
        label: tenant.exists ? tenant.display_name : `${tenant.display_name} (not created)`,
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
    clearInspection();
    setRouteSelection(null);
    changePageLocation(
      "sessions",
      sessionLocation(selectedScopes, selectedAgents),
      onLocationChange,
    );
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
      finishDeletion();
    }
  }

  async function deleteSelectedSessions() {
    if (!dialogKeys || dialogKeys.length === 0 || !beginDeletion({ kind: "batch" })) return;
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

  return (
    <div className={`${styles.page} ${styles.catalogPage} ${styles.sessionPage}`}>
      <PageError error={error} />
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
                    disabled={selectedKeys.size === 0 || deletionBusy}
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
                    disabled={deletionBusy}
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
                    label="Agent"
                    onCommit={commitAgents}
                    options={agentOptions}
                    pluralLabel="agents"
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
                  <button
                    ref={refreshButton}
                    data-dialog-focus-fallback="true"
                    type="button"
                    className={styles.sessionRefresh}
                    aria-label={refreshing ? "Refreshing Sessions" : "Refresh Sessions"}
                    aria-busy={refreshing}
                    title={refreshing ? "Refreshing Sessions" : "Refresh Sessions"}
                    disabled={loadingList || refreshing || deletionBusy}
                    onClick={() => void load("refresh")}
                  >
                    <RefreshCw
                      className={refreshing ? styles.spinning : undefined}
                      size={14}
                      aria-hidden="true"
                    />
                    Refresh
                  </button>
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
                      <strong>{title}</strong>
                      <small className={styles.sessionRowMetadata}>
                        <span>
                          {sessionListScopeLabel(row.source.scopeKey)} · {row.source.agentLabel}
                        </span>
                        <time dateTime={row.start_ts}>{formatTimestamp(row.start_ts)}</time>
                      </small>
                    </span>
                    {row.warnings.length > 0 && (
                      <AlertTriangle
                        className={styles.sessionRowWarning}
                        size={14}
                        aria-label="Session has Transcript warnings"
                      />
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
                      disabled={unsafeView || deletionBusy || loadingList}
                      onClick={() => void deleteSession(row)}
                    >
                      {deleting ? (
                        <LoaderCircle className={styles.spinning} size={15} aria-hidden="true" />
                      ) : (
                        <Trash2 size={15} aria-hidden="true" />
                      )}
                    </button>
                  )}
                </div>
              );
            })}
            {data?.sessions.length === 0 && !loadingList && (
              <div className={styles.sessionListEmpty}>
                <SessionIcon size={22} data-icon="session-list-empty" aria-hidden="true" />
                <strong>No Sessions found</strong>
                <p>No Sessions were found for the selected Tenants and Coding Agents.</p>
              </div>
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
                  <h2>{currentSession.title || "Untitled Session"}</h2>
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
                  <div className={styles.sessionEmptyPane}>
                    <SessionIcon size={24} aria-hidden="true" />
                    <h2>No typed prompts</h2>
                    <p>This Session&apos;s Transcript contains no supported typed user prompts.</p>
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className={styles.sessionEmptyPane}>
              <SessionIcon size={26} data-icon="session-empty" aria-hidden="true" />
              <h2>Select a Session</h2>
              <p>Choose a Session to inspect its prompts and Transcript warnings.</p>
            </div>
          )}
        </section>
      </div>
      <NotificationCenter
        notifications={notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogKeys !== null}
        onAction={() => undefined}
        onDismiss={dismissNotification}
      />
      {dialogKeys && (
        <DestructiveConfirmDialog
          title={`Delete ${dialogKeys.length} selected Session${dialogKeys.length === 1 ? "" : "s"}?`}
          message={`This permanently deletes the Transcripts for the selected Sessions. Sources: ${dialogSources
            .map(({ count, source }) => `${source.scopeLabel} · ${source.agentLabel} (${count})`)
            .join("; ")}.`}
          confirmLabel="Delete permanently"
          busy={batchBusy}
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
  onOperation,
  onDismiss,
}: {
  api: ControlApi;
  operation: Operation;
  onOperation: (operation: Operation) => void;
  onDismiss: () => void;
}) {
  async function cancel() {
    await api.post(`/_aibox/api/operations/${encodeURIComponent(operation.id)}/cancel`);
  }
  return (
    <section className={styles.operationPanel} aria-label="Management Operation">
      <header>
        <div>
          {operation.state === "running" ? (
            <LoaderCircle size={16} />
          ) : operation.state === "succeeded" ? (
            <Check size={16} />
          ) : (
            <CircleStop size={16} />
          )}
          <strong>{operation.kind}</strong>
        </div>
        <span>{operation.state}</span>
        {operation.state === "running" && (
          <IconButton label="Cancel operation" onClick={() => void cancel()}>
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
        <IconButton label="Dismiss operation" onClick={onDismiss}>
          <X size={15} />
        </IconButton>
      </header>
      <pre>
        {operation.logs.map((entry) => entry.message).join("\n") ||
          operation.result ||
          "Waiting for output"}
      </pre>
      {operation.result && <footer>{operation.result}</footer>}
    </section>
  );
}
