/* eslint-disable react-hooks/set-state-in-effect */

import {
  AlertTriangle,
  Archive,
  Box,
  Check,
  ChevronLeft,
  CircleStop,
  Database,
  Download,
  Eye,
  FileCode2,
  Hammer,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Save,
  Settings2,
  Sparkles,
  SquareTerminal,
  Trash2,
  UserRound,
  Wrench,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
import { NotificationCenter } from "./components/NotificationCenter";
import { useFailureNotifications } from "./useFailureNotifications";
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
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button className={styles.iconButton} type="button" title={label} aria-label={label} {...props}>
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
                {row.kind === "host" ? <UserRound size={16} /> : <Box size={16} />}
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

function ScopeControls({
  tenants,
  scope,
  agent,
  onScope,
  onAgent,
}: {
  tenants: TenantRow[];
  scope: Scope;
  agent: Agent;
  onScope: (scope: Scope) => void;
  onAgent: (agent: Agent) => void;
}) {
  const value = scope.scope === "host" ? "host" : `managed:${scope.tenant}`;
  return (
    <div className={styles.filters}>
      <label>
        Tenant
        <select
          value={value}
          onChange={(event) => {
            const next = event.target.value;
            onScope(
              next === "host" ? { scope: "host" } : { scope: "managed", tenant: next.slice(8) },
            );
          }}
        >
          {tenants.map((row) => (
            <option
              key={`${row.kind}:${row.name}`}
              value={row.kind === "host" ? "host" : `managed:${row.name}`}
            >
              {row.display_name}
            </option>
          ))}
        </select>
      </label>
      <div className={styles.segmented} aria-label="Coding Agent">
        {(["codex", "claude"] as const).map((value) => (
          <button
            type="button"
            key={value}
            aria-pressed={agent === value}
            onClick={() => onAgent(value)}
          >
            {value === "codex" ? "Codex" : "Claude"}
          </button>
        ))}
      </div>
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
  const [deleteName, setDeleteName] = useState<string | null>(null);
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);

  const loadCatalog = useCallback(async () => {
    const query = scopeQuery(scope);
    query.set("agent", agent);
    try {
      const data = await api.get<ConfigListData>(`/_aibox/api/configs?${query}`);
      setCatalog(data);
      setFile((current) =>
        current && data.files.includes(current) ? current : (data.files[0] ?? null),
      );
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, [agent, api, scope]);
  useEffect(() => {
    setSnapshot(null);
    setSelection({ current: true });
    void loadCatalog();
  }, [loadCatalog]);
  useEffect(() => setSnapshot(null), [file, selection]);

  const selectionBody = {
    ...scopeBody(scope),
    agent,
    current: selection.current,
    config: selection.current ? null : selection.config,
    file,
  };
  const selectedEntry = selection.current
    ? null
    : (catalog?.configs.find((entry) => entry.name === selection.config) ?? null);

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

  async function reveal() {
    if (!file) return;
    setBusy(true);
    try {
      const value = await api.post<ConfigFileData>("/_aibox/api/configs/reveal", selectionBody);
      const bytes = decodeBase64(value.content_base64);
      try {
        setEditor(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
        setEditorMode("text");
      } catch {
        setEditor(value.content_base64);
        setEditorMode("base64");
      }
      setSnapshot(value);
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!snapshot) return;
    setBusy(true);
    try {
      const bytes =
        editorMode === "text"
          ? new TextEncoder().encode(editor)
          : decodeBase64(editor.replace(/\s/g, ""));
      const value = await api.post<ConfigFileData>("/_aibox/api/configs/save", {
        ...selectionBody,
        revision: snapshot.revision,
        content_base64: encodeBase64(bytes),
      });
      setSnapshot(value);
      setError(null);
      await loadCatalog();
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function createConfig(name = newName) {
    if (!name) return;
    setBusy(true);
    try {
      await api.post("/_aibox/api/configs/create", { ...scopeBody(scope), agent, config: name });
      setNewName("");
      setSelection({ current: false, config: name });
      await loadCatalog();
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function applyConfig(name: string) {
    setBusy(true);
    try {
      await api.post("/_aibox/api/configs/apply", { ...scopeBody(scope), agent, config: name });
      await loadCatalog();
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  async function deleteConfig() {
    if (!deleteName) return;
    setBusy(true);
    try {
      await api.post("/_aibox/api/configs/delete", {
        ...scopeBody(scope),
        agent,
        configs: [deleteName],
        all: false,
        confirmation: deleteName,
      });
      setSelection({ current: true });
      setDeleteName(null);
      await loadCatalog();
    } catch (cause) {
      setError(messageOf(cause));
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
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={`${styles.page} ${styles.configPage}`}>
      <PageError error={error} />
      <div className={styles.pageToolbar}>
        <ScopeControls
          tenants={tenants}
          scope={scope}
          agent={agent}
          onScope={setScope}
          onAgent={setAgent}
        />
        {agent === "codex" && (
          <button type="button" onClick={() => void previewPropagation()} disabled={busy}>
            <Archive size={15} /> Propagate credentials
          </button>
        )}
      </div>
      <div className={styles.configLayout}>
        <aside className={styles.configCatalog}>
          <button
            type="button"
            className={selection.current ? styles.selectedRow : styles.catalogRow}
            onClick={() => setSelection({ current: true })}
          >
            <Settings2 size={16} />
            <span>
              <strong>Current</strong>
              <small>Native Config</small>
            </span>
          </button>
          <div className={styles.catalogDivider}>Named Configs</div>
          {catalog?.configs.map((entry) => (
            <button
              type="button"
              key={entry.name}
              title={entry.detail}
              className={
                !selection.current && selection.config === entry.name
                  ? styles.selectedRow
                  : styles.catalogRow
              }
              onClick={() => setSelection({ current: false, config: entry.name })}
            >
              <FileCode2 size={16} />
              <span>
                <strong>{entry.name}</strong>
                <small>{entry.state}</small>
              </span>
            </button>
          ))}
          <form
            className={styles.createRow}
            onSubmit={(event) => {
              event.preventDefault();
              void createConfig();
            }}
          >
            <input
              aria-label="New Config name"
              placeholder="config-name"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
            />
            <IconButton label="Create Named Config" type="submit" disabled={!newName || busy}>
              <Plus size={16} />
            </IconButton>
          </form>
        </aside>
        <section className={styles.configEditor}>
          {catalog && (
            <>
              <div className={styles.editorHeader}>
                <div>
                  <h2>{selection.current ? "Current Config" : selection.config}</h2>
                  <ApplicationBadge status={catalog.application} />
                </div>
                {!selection.current && (
                  <div className={styles.actionRow}>
                    {selectedEntry?.state === "incomplete" ? (
                      <button
                        className={styles.primaryButton}
                        disabled={busy}
                        onClick={() => void createConfig(selection.config)}
                      >
                        <Wrench size={15} /> Repair
                      </button>
                    ) : (
                      <button
                        className={styles.primaryButton}
                        disabled={busy || selectedEntry?.state !== "ready"}
                        onClick={() => void applyConfig(selection.config)}
                      >
                        <Check size={15} /> Apply
                      </button>
                    )}
                    <IconButton
                      label="Delete Named Config"
                      onClick={() => setDeleteName(selection.config)}
                    >
                      <Trash2 size={16} />
                    </IconButton>
                  </div>
                )}
              </div>
              <div className={styles.fileTabs} role="tablist">
                {catalog.files.map((name) => (
                  <button
                    type="button"
                    role="tab"
                    aria-selected={file === name}
                    key={name}
                    onClick={() => setFile(name)}
                  >
                    {name}
                  </button>
                ))}
              </div>
              {!snapshot ? (
                <div className={styles.revealPane}>
                  <Eye size={22} />
                  <button
                    className={styles.primaryButton}
                    type="button"
                    disabled={busy}
                    onClick={() => void reveal()}
                  >
                    Reveal {file}
                  </button>
                </div>
              ) : (
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
                      disabled={busy}
                      onClick={() => void save()}
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
              )}
            </>
          )}
        </section>
      </div>
      {deleteName && (
        <ConfirmDialog
          title={`Delete Named Config ${deleteName}?`}
          confirmation={deleteName}
          confirmLabel="Delete Config"
          busy={busy}
          onCancel={() => setDeleteName(null)}
          onConfirm={() => void deleteConfig()}
        />
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

function ApplicationBadge({ status }: { status: ApplicationStatus }) {
  return (
    <span
      className={
        status.drift === "clean"
          ? styles.goodStatus
          : status.drift === "untracked"
            ? styles.neutralStatus
            : styles.warnStatus
      }
      title={status.detail}
    >
      {status.last_application
        ? `${status.last_application.applied} · ${status.drift}`
        : status.drift}
    </span>
  );
}

type SessionDeletion = { kind: "record"; id: string } | { kind: "batch" } | null;

function sessionRequestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

function focusTargetAfterSessionDelete(rows: SessionRow[], id: string): string | null {
  const index = rows.findIndex((row) => row.id === id);
  if (index < 0) return null;
  return rows[index + 1]?.id ?? rows[index - 1]?.id ?? null;
}

export function SessionPage({ api }: PageProps) {
  const tenants = useTenants(api);
  const [scope, setScope] = useState<Scope>({ scope: "managed", tenant: "default" });
  const [agent, setAgent] = useState<Agent>("codex");
  const [data, setData] = useState<SessionListData | null>(null);
  const [currentSession, setCurrentSession] = useState<SessionRow | null>(null);
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [promptWarnings, setPromptWarnings] = useState<string[]>([]);
  const [loadingPrompts, setLoadingPrompts] = useState(false);
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [dialogIds, setDialogIds] = useState<string[] | null>(null);
  const [deletion, setDeletion] = useState<SessionDeletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const listController = useRef<AbortController | null>(null);
  const streamController = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SessionRow | null>(null);
  const deletionInFlight = useRef(false);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const deleteButtons = useRef(new Map<string, HTMLButtonElement>());
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();

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
    async (kind: "initial" | "refresh" = "initial"): Promise<SessionListData | null> => {
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
      const query = scopeQuery(scope);
      query.set("agent", agent);
      try {
        const result = await api.get<SessionListData>(
          `/_aibox/api/sessions?${query}`,
          controller.signal,
        );
        if (listController.current !== controller || controller.signal.aborted) return null;
        setData(result);
        setError(null);
        const inspected = currentSessionRef.current;
        if (inspected) {
          const refreshed = result.sessions.find((row) => row.id === inspected.id);
          if (refreshed) {
            currentSessionRef.current = refreshed;
            setCurrentSession(refreshed);
          } else {
            clearInspection();
          }
        }
        if (result.warnings.length > 0) {
          setSelectedIds(new Set());
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
    [agent, api, clearInspection, scope],
  );

  useEffect(() => {
    clearInspection();
    setData(null);
    setError(null);
    setSelectionMode(false);
    setSelectedIds(new Set());
    setDialogIds(null);
    setFocusAfterDelete(undefined);
    void load();
    return () => {
      listController.current?.abort();
      abortPromptStream();
    };
  }, [abortPromptStream, clearInspection, load]);

  useEffect(() => {
    if (focusAfterDelete === undefined || deletion !== null) return;
    const preferred = focusAfterDelete ? deleteButtons.current.get(focusAfterDelete) : null;
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (target && !target.disabled) {
      target.focus();
      setFocusAfterDelete(undefined);
    }
  }, [data, deletion, focusAfterDelete]);

  async function openSession(row: SessionRow) {
    abortPromptStream();
    const controller = new AbortController();
    streamController.current = controller;
    currentSessionRef.current = row;
    setCurrentSession(row);
    setPrompts([]);
    setPromptWarnings([]);
    setLoadingPrompts(true);
    const query = scopeQuery(scope);
    query.set("agent", agent);
    query.set("id", row.id);
    try {
      const result = await api.streamSession(
        `/_aibox/api/sessions/prompts?${query}`,
        (prompt) => setPrompts((current) => [...current, prompt]),
        controller.signal,
      );
      if (streamController.current === controller) setPromptWarnings(result.warnings);
    } catch (cause) {
      if (!sessionRequestCancelled(cause, controller.signal)) setError(messageOf(cause));
    } finally {
      if (streamController.current === controller) {
        streamController.current = null;
        setLoadingPrompts(false);
      }
    }
  }

  function toggleSession(id: string) {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  function toggleAllSessions() {
    const ids = data?.sessions.map((row) => row.id) ?? [];
    const allSelected = ids.length > 0 && ids.every((id) => selectedIds.has(id));
    setSelectedIds(allSelected ? new Set() : new Set(ids));
  }

  function cancelSelection() {
    setSelectionMode(false);
    setSelectedIds(new Set());
  }

  async function requestSessionDeletion(ids: string[]) {
    return api.post<{ deleted: number }>("/_aibox/api/sessions/delete", {
      ...scopeBody(scope),
      agent,
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

  async function deleteSession(row: SessionRow) {
    if (data?.warnings.length || !data || !beginDeletion({ kind: "record", id: row.id })) return;
    const originRows = data.sessions;
    const wasCurrent = currentSessionRef.current?.id === row.id;
    if (wasCurrent) abortPromptStream();
    resolveFailure("action");
    try {
      await requestSessionDeletion([row.id]);
      setData((current) =>
        current
          ? { ...current, sessions: current.sessions.filter((session) => session.id !== row.id) }
          : current,
      );
      if (wasCurrent) clearInspection();
      await load("refresh");
      setFocusAfterDelete(focusTargetAfterSessionDelete(originRows, row.id));
    } catch (cause) {
      reportFailure("action", "Couldn’t delete Session", cause);
      const refreshed = await load("refresh");
      const survivor = refreshed?.sessions.find((session) => session.id === row.id);
      if (wasCurrent && survivor) void openSession(survivor);
      setFocusAfterDelete(survivor ? row.id : null);
    } finally {
      finishDeletion();
    }
  }

  async function deleteSelectedSessions() {
    if (!dialogIds || dialogIds.length === 0 || !beginDeletion({ kind: "batch" })) return;
    const ids = dialogIds;
    const idSet = new Set(ids);
    const currentId = currentSessionRef.current?.id;
    const wasCurrent = currentId ? idSet.has(currentId) : false;
    if (wasCurrent) abortPromptStream();
    resolveFailure("action");
    try {
      await requestSessionDeletion(ids);
      setData((current) =>
        current
          ? { ...current, sessions: current.sessions.filter((session) => !idSet.has(session.id)) }
          : current,
      );
      if (wasCurrent) clearInspection();
      setSelectedIds(new Set());
      setSelectionMode(false);
      setDialogIds(null);
      await load("refresh");
      setFocusAfterDelete(null);
    } catch (cause) {
      setDialogIds(null);
      reportFailure("action", "Couldn’t delete Sessions", cause);
      const refreshed = await load("refresh");
      if (refreshed && refreshed.warnings.length === 0) {
        const remaining = new Set(
          ids.filter((id) => refreshed.sessions.some((row) => row.id === id)),
        );
        setSelectedIds(remaining);
        setSelectionMode(remaining.size > 0);
        if (wasCurrent && currentId) {
          const survivor = refreshed.sessions.find((row) => row.id === currentId);
          if (survivor) void openSession(survivor);
        }
      }
    } finally {
      finishDeletion();
    }
  }

  const unsafeView = (data?.warnings.length ?? 0) > 0;
  const sessions = data?.sessions ?? [];
  const allSelected = sessions.length > 0 && sessions.every((row) => selectedIds.has(row.id));
  const deletionBusy = deletion !== null;
  const hasDefaultTenant = tenants.some(
    (tenant) => tenant.kind === "managed" && tenant.name === "default",
  );
  const scopeValue = scope.scope === "host" ? "host" : `managed:${scope.tenant}`;
  const batchBusy = deletion?.kind === "batch";

  return (
    <div className={`${styles.page} ${styles.catalogPage} ${styles.sessionPage}`}>
      <PageError error={error} />
      <div className={`${styles.splitLayout} ${currentSession ? styles.hasSelection : ""}`}>
        <aside className={`${styles.catalog} ${styles.sessionCatalog}`} aria-label="Sessions">
          <div className={styles.sessionToolbar}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={styles.sessionSelectAll}
                  onClick={toggleAllSessions}
                  disabled={sessions.length === 0 || deletionBusy}
                >
                  {allSelected ? "Clear all" : "Select all"}
                </button>
                <span
                  className={styles.sessionSelectionCount}
                  title={`${selectedIds.size} selected`}
                >
                  {selectedIds.size} selected
                </span>
                <IconButton
                  label="Delete selected Sessions"
                  disabled={selectedIds.size === 0 || deletionBusy}
                  onClick={() => setDialogIds([...selectedIds])}
                >
                  <Trash2 size={15} aria-hidden="true" />
                </IconButton>
                <IconButton
                  label="Cancel Session selection"
                  disabled={deletionBusy}
                  onClick={cancelSelection}
                >
                  <X size={16} aria-hidden="true" />
                </IconButton>
              </>
            ) : (
              <>
                <label className={styles.sessionTenant}>
                  <span className="srOnly">Tenant</span>
                  <select
                    aria-label="Tenant"
                    value={scopeValue}
                    disabled={deletionBusy}
                    onChange={(event) => {
                      const next = event.target.value;
                      setScope(
                        next === "host"
                          ? { scope: "host" }
                          : { scope: "managed", tenant: next.slice(8) },
                      );
                    }}
                  >
                    {!hasDefaultTenant && (
                      <option value="managed:default">default (not created)</option>
                    )}
                    {tenants.map((tenant) => (
                      <option
                        key={`${tenant.kind}:${tenant.name}`}
                        value={tenant.kind === "host" ? "host" : `managed:${tenant.name}`}
                      >
                        {tenant.display_name}
                      </option>
                    ))}
                  </select>
                </label>
                <div className={styles.sessionAgents} role="group" aria-label="Coding Agent">
                  <button
                    type="button"
                    title="Codex"
                    aria-label="Codex"
                    aria-pressed={agent === "codex"}
                    disabled={deletionBusy}
                    onClick={() => setAgent("codex")}
                  >
                    <SquareTerminal size={16} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    title="Claude"
                    aria-label="Claude"
                    aria-pressed={agent === "claude"}
                    disabled={deletionBusy}
                    onClick={() => setAgent("claude")}
                  >
                    <Sparkles size={16} aria-hidden="true" />
                  </button>
                </div>
                <button
                  ref={refreshButton}
                  data-dialog-focus-fallback="true"
                  type="button"
                  className={styles.iconButton}
                  title="Refresh Sessions"
                  aria-label={refreshing ? "Refreshing Sessions" : "Refresh Sessions"}
                  aria-busy={refreshing}
                  disabled={loadingList || refreshing || deletionBusy}
                  onClick={() => void load("refresh")}
                >
                  <RefreshCw
                    className={refreshing ? styles.spinning : undefined}
                    size={16}
                    aria-hidden="true"
                  />
                </button>
                <IconButton
                  label="Select Sessions"
                  disabled={
                    sessions.length === 0 || unsafeView || loadingList || refreshing || deletionBusy
                  }
                  onClick={() => setSelectionMode(true)}
                >
                  <ListChecks size={16} aria-hidden="true" />
                </IconButton>
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
              const selectedForDeletion = selectedIds.has(row.id);
              const deleting = deletion?.kind === "record" && deletion.id === row.id;
              const title = row.title || "Untitled Session";
              return (
                <div
                  key={row.id}
                  className={[
                    styles.sessionRow,
                    currentSession?.id === row.id ? styles.currentSessionRow : "",
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
                        ? `${selectedForDeletion ? "Deselect" : "Select"} ${title}`
                        : title
                    }
                    aria-pressed={selectionMode ? selectedForDeletion : undefined}
                    disabled={deletionBusy || loadingList}
                    onClick={() => (selectionMode ? toggleSession(row.id) : void openSession(row))}
                  >
                    <Database size={16} aria-hidden="true" />
                    <span>
                      <strong>{title}</strong>
                      <small>
                        {row.display_id} · {row.start_ts || "No timestamp"}
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
                        if (element) deleteButtons.current.set(row.id, element);
                        else deleteButtons.current.delete(row.id);
                      }}
                      type="button"
                      className={styles.sessionDelete}
                      title={`Delete Session ${row.display_id}`}
                      aria-label={
                        deleting
                          ? `Deleting Session ${row.display_id}`
                          : `Delete Session ${row.display_id}`
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
            {data?.sessions.length === 0 && <div className={styles.emptyList}>No Sessions</div>}
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
                  <code>{currentSession.id}</code>
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
                  <div className={styles.emptyPane}>No typed prompts</div>
                )}
              </div>
            </>
          ) : (
            <div className={styles.emptyPane}>
              <Database size={24} />
              <span>Select a Session</span>
            </div>
          )}
        </section>
      </div>
      <NotificationCenter
        notifications={notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogIds !== null}
        onAction={() => undefined}
        onDismiss={dismissNotification}
      />
      {dialogIds && (
        <DestructiveConfirmDialog
          title={`Delete ${dialogIds.length} selected Session${dialogIds.length === 1 ? "" : "s"}?`}
          message={
            scope.scope === "host"
              ? `This permanently deletes the Transcripts for the selected Sessions from the real Host Home for ${agent === "codex" ? "Codex" : "Claude"}.`
              : `This permanently deletes the Transcripts for the selected Sessions in Tenant ${scope.tenant} for ${agent === "codex" ? "Codex" : "Claude"}.`
          }
          confirmLabel="Delete permanently"
          busy={batchBusy}
          onCancel={() => {
            if (!batchBusy) setDialogIds(null);
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
