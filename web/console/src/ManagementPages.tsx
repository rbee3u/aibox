/* eslint-disable react-hooks/set-state-in-effect */

import {
  AlertTriangle,
  Box,
  Check,
  ChevronDown,
  ChevronLeft,
  CircleStop,
  Container,
  Download,
  FileClock,
  FileCog,
  FileCode2,
  Hammer,
  House,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  ControlApi,
  decodeBase64,
  encodeBase64,
  formatBytes,
  scopeBody,
  scopeQuery,
} from "./controlApi";
import type {
  Agent,
  ApplicationStatus,
  ComponentRow,
  ConfigCatalogEntry,
  ConfigFileData,
  ConfigListData,
  Operation,
  OverviewData,
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
import styles from "./ManagementPages.module.css";

interface PageProps {
  api: ControlApi;
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
  confirmation,
  confirmLabel,
  busy,
  onCancel,
  onConfirm,
}: {
  title: string;
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

function Metric({ label, value, detail }: { label: string; value: ReactNode; detail?: string }) {
  return (
    <div className={styles.metric}>
      <span>{label}</span>
      <strong>{value}</strong>
      {detail && <small title={detail}>{detail}</small>}
    </div>
  );
}

export function OverviewPage({ api, onOperation }: PageProps) {
  const [data, setData] = useState<OverviewData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    try {
      setData(await api.get<OverviewData>("/_aibox/api/overview"));
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, [api]);
  useEffect(() => void load(), [load]);

  async function build(force: boolean) {
    setBusy(true);
    try {
      const operation = await api.post<Operation>("/_aibox/api/operations/build", { force });
      onOperation?.(operation);
      setData((current) => (current ? { ...current, operation } : current));
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  if (!data && !error) return <Loading />;
  return (
    <div className={styles.page}>
      <PageError error={error} />
      <div className={styles.pageToolbar}>
        <div />
        <IconButton label="Refresh status" onClick={() => void load()}>
          <RefreshCw size={16} />
        </IconButton>
      </div>
      {data && (
        <>
          <section className={styles.metricGrid} aria-label="Service status">
            <Metric label="Service" value="Running" detail={`${data.listen} · ${data.version}`} />
            <Metric
              label="Docker"
              value={data.docker === "available" ? "Available" : "Unavailable"}
              detail={data.docker_error ?? undefined}
            />
            <Metric label="Managed Tenants" value={data.managed_tenants} />
            <Metric
              label="Requests"
              value={data.request_records}
              detail={formatBytes(data.request_bytes)}
            />
          </section>
          <section className={styles.band}>
            <div className={styles.bandHeading}>
              <div>
                <h2>Runtime image</h2>
                <code>{data.runtime_image}</code>
              </div>
              <span className={data.image_available ? styles.goodStatus : styles.warnStatus}>
                {data.image_available ? "Built" : "Missing"}
              </span>
            </div>
            <div className={styles.actionRow}>
              <button
                className={styles.primaryButton}
                onClick={() => void build(false)}
                disabled={busy}
              >
                <Hammer size={15} /> Build
              </button>
              <button onClick={() => void build(true)} disabled={busy}>
                <RefreshCw size={15} /> Rebuild without cache
              </button>
            </div>
          </section>
          <section className={styles.band}>
            <h2>Storage</h2>
            <dl className={styles.details}>
              <dt>aibox Root</dt>
              <dd>
                <code>{data.aibox_root}</code>
              </dd>
              <dt>Uptime</dt>
              <dd>{formatDuration(data.uptime_seconds)}</dd>
            </dl>
          </section>
        </>
      )}
    </div>
  );
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours) return `${hours}h ${minutes}m`;
  if (minutes) return `${minutes}m ${seconds % 60}s`;
  return `${seconds}s`;
}

function tenantScope(row: TenantRow): Scope {
  return row.kind === "host" ? { scope: "host" } : { scope: "managed", tenant: row.name! };
}

export function TenantPage({ api, onOperation }: PageProps) {
  const [tenants, setTenants] = useState<TenantRow[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [components, setComponents] = useState<ComponentRow[]>([]);
  const [versions, setVersions] = useState<Record<string, string>>({});
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<TenantRow | null>(null);
  const keyOf = (row: TenantRow) => (row.kind === "host" ? "host" : `managed:${row.name}`);
  const selected = tenants.find((row) => keyOf(row) === selectedKey) ?? null;

  const loadTenants = useCallback(async () => {
    try {
      const rows = await api.get<TenantRow[]>("/_aibox/api/tenants");
      setTenants(rows);
      setSelectedKey(
        (current) =>
          current ??
          (rows.find((row) => row.name === "default") ? "managed:default" : keyOf(rows[0])),
      );
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, [api]);
  useEffect(() => void loadTenants(), [loadTenants]);

  const loadComponents = useCallback(async () => {
    if (!selected) return;
    try {
      const query = scopeQuery(tenantScope(selected));
      setComponents(await api.get<ComponentRow[]>(`/_aibox/api/components?${query}`));
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, [api, selected]);
  useEffect(() => void loadComponents(), [loadComponents]);

  async function createTenant() {
    if (!newName) return;
    setBusy(true);
    try {
      await api.post("/_aibox/api/tenants", { name: newName });
      const created = newName;
      setNewName("");
      await loadTenants();
      setSelectedKey(`managed:${created}`);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function deleteTenant() {
    if (!deleteTarget?.name) return;
    setBusy(true);
    try {
      await api.post("/_aibox/api/tenants/delete", {
        names: [deleteTarget.name],
        all: false,
        confirmation: deleteTarget.name,
      });
      setDeleteTarget(null);
      setSelectedKey(null);
      await loadTenants();
    } catch (cause) {
      setError(messageOf(cause));
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
      <div className={`${styles.splitLayout} ${selected ? styles.hasSelection : ""}`}>
        <aside className={styles.catalog} aria-label="Tenants">
          <form
            className={styles.createRow}
            onSubmit={(event) => {
              event.preventDefault();
              void createTenant();
            }}
          >
            <input
              aria-label="New Tenant name"
              placeholder="tenant-name"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
            />
            <IconButton label="Create Tenant" type="submit" disabled={busy || !newName}>
              <Plus size={16} />
            </IconButton>
          </form>
          <div className={styles.catalogList}>
            {tenants.map((row) => (
              <button
                className={keyOf(row) === selectedKey ? styles.selectedRow : styles.catalogRow}
                type="button"
                key={keyOf(row)}
                onClick={() => setSelectedKey(keyOf(row))}
              >
                {row.kind === "host" ? (
                  <House size={16} data-icon="host-tenant" />
                ) : (
                  <Container size={16} data-icon="managed-tenant" />
                )}
                <span>
                  <strong>{row.display_name}</strong>
                  <small>{row.kind}</small>
                </span>
              </button>
            ))}
          </div>
        </aside>
        <section className={styles.detailPane}>
          {selected ? (
            <>
              <div className={styles.detailHeader}>
                <IconButton label="Back to Tenants" onClick={() => setSelectedKey(null)}>
                  <ChevronLeft size={17} />
                </IconButton>
                <div>
                  <h2>{selected.display_name}</h2>
                  <code>{selected.home}</code>
                </div>
                {selected.kind === "managed" && (
                  <IconButton label="Delete Tenant" onClick={() => setDeleteTarget(selected)}>
                    <Trash2 size={16} />
                  </IconButton>
                )}
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
                    <div className={styles.componentRow} key={row.kind}>
                      <Wrench size={17} aria-hidden="true" />
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
      {deleteTarget?.name && (
        <ConfirmDialog
          title={`Delete Tenant ${deleteTarget.name}?`}
          confirmation={deleteTarget.name}
          confirmLabel="Delete Tenant"
          busy={busy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteTenant()}
        />
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

export function ConfigPage({ api }: PageProps) {
  const tenants = useTenants(api);
  const [scope, setScope] = useState<Scope>({ scope: "managed", tenant: "default" });
  const [agent, setAgent] = useState<Agent>("codex");
  const [catalog, setCatalog] = useState<ConfigListData | null>(null);
  const [selection, setSelection] = useState<ConfigSelection>({ current: true });
  const [file, setFile] = useState<string | null>(null);
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
  const [detailOpen, setDetailOpen] = useState(false);
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);
  const catalogController = useRef<AbortController | null>(null);
  const fileLoadGeneration = useRef(0);

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
              icon: <House size={14} aria-hidden="true" />,
            },
          ]
        : []),
      ...managed.map((tenant) => ({
        value: `managed:${tenant.name}` as const,
        label: tenant.exists ? tenant.display_name : `${tenant.display_name} (not created)`,
        summaryLabel: tenant.display_name,
        icon: <Container size={14} aria-hidden="true" />,
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
    setSelection({ current: true });
    setSelectionMode(false);
    setSelectedNames(new Set());
    setDetailOpen(false);
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
    editorBytes !== null &&
    encodeBase64(editorBytes) !== snapshot.content_base64;

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
    });
  }

  function openConfig(name: string) {
    requestEditorAction(() => {
      setSelection({ current: false, config: name });
      setDetailOpen(true);
    });
  }

  function openCurrent() {
    requestEditorAction(() => {
      setSelection({ current: true });
      setDetailOpen(true);
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
                    triggerIcon={<Container size={14} aria-hidden="true" />}
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
                  <FileCog size={16} data-icon="current-config" />
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
                  disabled={busy || loadingCatalog}
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
                      <FileCode2 size={16} />
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
                <IconButton label="Back to Configs" onClick={() => setDetailOpen(false)}>
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
                        onClick={() => requestEditorAction(() => setFile(name))}
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
                  <FileCode2 size={22} />
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
          confirmation={deleteTarget.names[0]}
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

export function SessionPage({ api }: PageProps) {
  const tenants = useTenants(api);
  const [selectedScopes, setSelectedScopes] = useState<Set<SessionScopeKey>>(
    () => new Set(["managed:default"]),
  );
  const [selectedAgents, setSelectedAgents] = useState<Set<Agent>>(() => new Set(["codex"]));
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
              icon: <House size={14} aria-hidden="true" />,
            },
          ]
        : []),
      ...managed.map((tenant) => ({
        value: `managed:${tenant.name}` as const,
        label: tenant.exists ? tenant.display_name : `${tenant.display_name} (not created)`,
        summaryLabel: tenant.display_name,
        icon: <Container size={14} aria-hidden="true" />,
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

  async function openSession(row: SourcedSession) {
    abortPromptStream();
    const controller = new AbortController();
    streamController.current = controller;
    currentSessionRef.current = row;
    setCurrentSession(row);
    setPrompts([]);
    setPromptWarnings([]);
    setLoadingPrompts(true);
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
  }

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
    setSelectedScopes(new Set(values));
  }

  function commitAgents(values: ReadonlySet<Agent>) {
    setSelectedAgents(new Set(values));
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
                    triggerIcon={<Container size={14} aria-hidden="true" />}
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
                    <FileClock size={16} data-icon="session-record" aria-hidden="true" />
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
                <FileClock size={22} data-icon="session-list-empty" aria-hidden="true" />
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
                <IconButton label="Back to Sessions" onClick={clearInspection}>
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
                    <FileClock size={24} aria-hidden="true" />
                    <h2>No typed prompts</h2>
                    <p>This Session&apos;s Transcript contains no supported typed user prompts.</p>
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className={styles.sessionEmptyPane}>
              <FileClock size={26} data-icon="session-empty" aria-hidden="true" />
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
