import {
  Check,
  ChevronLeft,
  Download,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { ComponentRow, Operation, TenantApi, TenantSelection, TenantRow } from "./controlApi";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ActionButton } from "./components/ActionButton";
import { Dialog } from "./components/Dialog";
import { EmptyState } from "./components/EmptyState";
import { TextInput } from "./components/FormControls";
import { IconButton } from "./components/IconButton";
import { Loading, MutationUnavailable, PageError } from "./components/ManagementFeedback";
import { resourceIcons, type ModuleId } from "./consoleIcons";
import {
  changePageLocation,
  DNS_LABEL_PATTERN,
  messageOf,
  parseTenantSelectionKey,
} from "./managementSupport";
import styles from "./TenantPage.module.css";
const ComponentIcon = resourceIcons.component;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
interface PageProps {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange?: (module: ModuleId, query: URLSearchParams, replace?: boolean) => void;
  onOperation?: (operation: Operation) => void;
}
function tenantSelection(row: TenantRow): TenantSelection {
  return row.kind === "host" ? { kind: "host" } : { kind: "managed", name: row.name };
}
type TenantKey = "host" | `managed:${string}`;
type TenantDeleteTarget = {
  names: string[];
};
type ComponentRemoveTarget = {
  row: ComponentRow;
  tenantLabel: string;
};
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
function tenantLocation(key: TenantKey | null, component?: string | null): URLSearchParams {
  const query = new URLSearchParams();
  if (key) query.set("tenant", key);
  if (key && component) query.set("component", component);
  return query;
}
function abbreviateTenantHome(path: string, hostHome: string | null): string {
  if (!hostHome) return path;
  if (path === hostHome) return "~";
  const prefix = hostHome.endsWith("/") ? hostHome : `${hostHome}/`;
  return path.startsWith(prefix) ? `~/${path.slice(prefix.length)}` : path;
}
export function TenantPage({ api, operation, search, onLocationChange, onOperation }: PageProps) {
  const [initialRoute] = useState(() => new URLSearchParams(search));
  const observedSearch = useRef(search);
  const initialKey = parseTenantSelectionKey(initialRoute.get("tenant"));
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
  const mutationBusy = busy || operation?.state === "running";
  useEffect(() => {
    if (observedSearch.current === search) return;
    observedSearch.current = search;
    const query = new URLSearchParams(search);
    const key = parseTenantSelectionKey(query.get("tenant"));
    setSelectedKey(key);
    setSelectedComponent(key ? query.get("component") : null);
    setDetailOpen(key !== null);
  }, [search]);
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
  useEffect(() => {
    // The page lifecycle synchronizes with the external Tenant catalog.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadTenants();
  }, [loadTenants]);
  const loadComponents = useCallback(async () => {
    if (!selected) {
      setComponents([]);
      return;
    }
    try {
      const rows = await api.listComponents(tenantSelection(selected));
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
  useEffect(() => {
    // The selected Tenant determines which external Component catalog is loaded.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadComponents();
  }, [loadComponents]);
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
      await api.createTenant(newName);
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
  async function mutateComponent(row: ComponentRow, install: boolean) {
    if (!selected) return;
    setBusy(true);
    try {
      const result = await api.mutateComponent(
        tenantSelection(selected),
        row.kind,
        install,
        versions[row.kind] || null,
      );
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
      <div className={`${styles.splitLayout} ${detailOpen ? styles.hasSelection : ""}`}>
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
                        <TextInput
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
