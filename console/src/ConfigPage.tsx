import {
  AlertTriangle,
  Check,
  ChevronLeft,
  Download,
  Eye,
  EyeOff,
  ListChecks,
  LoaderCircle,
  Plus,
  RefreshCw,
  Save,
  Trash2,
} from "lucide-react";
import { basicSetup, EditorView } from "codemirror";
import { json } from "@codemirror/lang-json";
import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { tags } from "@lezer/highlight";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { decodeBase64, encodeBase64 } from "./controlApi";
import type {
  CodingAgentKind,
  ApplicationStatus,
  ConfigApi,
  ConfigAuthData,
  ConfigCatalogEntry,
  ConfigFileData,
  ConfigVisualOption,
  ConfigCustomProvider,
  ConfigListData,
  Operation,
  PropagationPreview,
  PropagationReport,
  PropagationOutcome,
  TenantSelection,
  TenantRow,
} from "./controlApi";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ActionButton } from "./components/ActionButton";
import { Dialog } from "./components/Dialog";
import { EmptyState } from "./components/EmptyState";
import { NativeSelect, TextArea, TextInput, Toggle } from "./components/FormControls";
import { IconButton } from "./components/IconButton";
import { Loading, MutationUnavailable, PageError } from "./components/ManagementFeedback";
import { SelectionMenu, type SelectionOption } from "./components/SelectionMenu";
import { HelpTooltip, IssueIndicator, type IssueTone } from "./components/IssueIndicator";
import { AgentIcon } from "./icons";
import { resourceIcons, type ModuleId } from "./consoleIcons";
import {
  changePageLocation,
  DNS_LABEL_PATTERN,
  messageOf,
  parseTenantSelectionKey,
  useTenants,
} from "./managementSupport";
import styles from "./ConfigPage.module.css";
const CurrentConfigIcon = resourceIcons.currentConfig;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
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
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange?: (module: ModuleId, query: URLSearchParams, replace?: boolean) => void;
  onOperation?: (operation: Operation) => void;
}
type ConfigSelection =
  | {
      current: true;
      config?: never;
    }
  | {
      current: false;
      config: string;
    };
type ConfigTenantKey = "host" | `managed:${string}`;
type ConfigDeleteTarget = {
  names: string[];
};
type ConfigApplyTarget = {
  name: string;
};
type ConfigPendingAction = {
  run: () => void | Promise<void>;
};
function configTenantKey(tenant: TenantSelection): ConfigTenantKey {
  return tenant.kind === "host" ? "host" : `managed:${tenant.name}`;
}
function tenantSelectionFromConfigKey(key: ConfigTenantKey): TenantSelection {
  return key === "host" ? { kind: "host" } : { kind: "managed", name: key.slice(8) };
}
interface ConfigRouteState {
  tenant: TenantSelection;
  agent: CodingAgentKind;
  selection: ConfigSelection;
  file: string | null;
  detailOpen: boolean;
}
function readConfigRoute(search: string): ConfigRouteState {
  const query = new URLSearchParams(search);
  const tenantKey = parseTenantSelectionKey(query.get("tenant")) ?? "managed:default";
  const agent = query.get("agent") === "claude" ? "claude" : "codex";
  const config = query.get("config");
  const current = query.get("current") === "1";
  const detailOpen = current || (config !== null && DNS_LABEL_PATTERN.test(config));
  return {
    tenant: tenantSelectionFromConfigKey(tenantKey),
    agent,
    selection:
      !current && config && DNS_LABEL_PATTERN.test(config)
        ? { current: false, config }
        : { current: true },
    file: detailOpen ? query.get("file") : null,
    detailOpen,
  };
}
function configLocation(
  tenant: TenantSelection,
  agent: CodingAgentKind,
  selection: ConfigSelection | null,
  file?: string | null,
): URLSearchParams {
  const query = new URLSearchParams();
  query.set("tenant", configTenantKey(tenant));
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
type ConfigFileController = {
  dirty: boolean;
  canSave: boolean;
  save: () => Promise<boolean>;
  restore: () => void;
  reload: () => void;
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
function configWarningPresentation(entry: ConfigCatalogEntry): ConfigIssuePresentation | null {
  if (entry.state !== "ready" || !entry.warnings?.length) return null;
  const message = entry.warnings.join(" ");
  return {
    tone: "warning",
    label: "Config warnings",
    message,
    accessibleLabel: `Config warning: ${message}`,
  };
}
function configIssueDescriptionId(
  tenant: TenantSelection,
  agent: CodingAgentKind,
  name: string,
): string {
  return `config-issue-${configTenantKey(tenant).replace(":", "-")}-${agent}-${name}`;
}
function propagationGroup(
  status: PropagationOutcome["status"],
): "updated" | "skipped" | "attention" {
  if (status === "updated") return "updated";
  if (status === "unchanged") return "skipped";
  return "attention";
}
function propagationDetail(outcome: PropagationOutcome): string | null {
  switch (outcome.status) {
    case "newer":
      return `source ${outcome.source_last_refresh} · target ${outcome.target_last_refresh}`;
    case "conflict":
      return `last refresh ${outcome.last_refresh}`;
    case "invalid":
    case "failed":
      return outcome.reason;
    default:
      return null;
  }
}
function requestProxyRoute(tenant: TenantSelection, listen: string | undefined): string | null {
  const port = listen?.match(/:(\d+)$/)?.[1];
  if (!port || port === "0") return null;
  return tenant.kind === "host"
    ? `http://127.0.0.1:${port}/`
    : `http://host.docker.internal:${port}/`;
}
function splitRequestProxyValue(
  value: string,
  route: string | null,
): {
  upstream: string;
  routed: boolean;
} {
  if (!value || !route) return { upstream: value, routed: false };
  const knownRoute = /^https?:\/\/(?:127\.0\.0\.1|host\.docker\.internal):(\d+)\//i;
  const match = value.match(knownRoute);
  if (!match || match[1] === "0") return { upstream: value, routed: false };
  return { upstream: value.slice(match[0].length), routed: true };
}
function comparableProvider(
  provider: ConfigCustomProvider | undefined,
): Pick<ConfigCustomProvider, "included" | "name" | "base_url"> | null {
  if (!provider) return null;
  return {
    included: provider.included,
    name: provider.name,
    base_url: provider.base_url,
  };
}
function proxyValueIsValid(value: string): boolean {
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:") && Boolean(url.hostname);
  } catch {
    return false;
  }
}
function VisualOptionLabel({
  id,
  label,
  description,
  required,
}: {
  id: string;
  label: string;
  description: string;
  required: boolean;
}) {
  return (
    <div className={styles.visualOptionLabel}>
      <label htmlFor={id}>{label}</label>
      <HelpTooltip label={label} message={description} />
      {required && (
        <>
          <span className={styles.requiredMarker} aria-hidden="true">
            *
          </span>
          <span className="srOnly">required</span>
        </>
      )}
    </div>
  );
}
function VisualConfigOptions({
  fields,
  provider,
  onChange,
  onProviderChange,
  tenant,
  listen,
}: {
  fields: ConfigVisualOption[];
  provider?: ConfigCustomProvider;
  onChange: (path: string, update: Partial<ConfigVisualOption>) => void;
  onProviderChange?: (update: Partial<ConfigCustomProvider>) => void;
  tenant?: TenantSelection;
  listen?: string;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const groups = useMemo(() => {
    const grouped = new Map<string, ConfigVisualOption[]>();
    for (const field of fields)
      grouped.set(field.group, [...(grouped.get(field.group) ?? []), field]);
    return [...grouped.entries()];
  }, [fields]);
  const customProviderSelected = Boolean(provider?.included);
  return (
    <div className={styles.visualEditor}>
      {groups.map(([group, groupFields]) => (
        <section className={styles.visualGroup} key={group}>
          <header>
            <h3>{group}</h3>
          </header>
          <div className={styles.visualFieldList}>
            {groupFields.map((field) => {
              const rawValue = field.value ?? (field.value_kind === "bool" ? false : "");
              const fieldId = `config-option-${field.path.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
              const isRevealed = revealed.has(field.path);
              const proxyRoute = field.request_proxy_route
                ? requestProxyRoute(tenant ?? { kind: "managed", name: "default" }, listen)
                : null;
              const split =
                field.request_proxy_route && typeof rawValue === "string"
                  ? splitRequestProxyValue(rawValue, proxyRoute)
                  : { upstream: rawValue, routed: Boolean(field.proxy_routed) };
              const value = split.upstream;
              const routed = Boolean(field.proxy_routed) || split.routed;
              const hasEnumValues = field.enum_values.length > 0;
              const customProviderField = field.path.startsWith("model_providers.custom.");
              const required =
                Boolean(field.required) || (customProviderSelected && customProviderField);
              const included = field.included || (customProviderSelected && customProviderField);
              const unsupportedValue =
                hasEnumValues &&
                included &&
                typeof value === "string" &&
                !field.enum_values.includes(value)
                  ? value
                  : null;
              return (
                <article className={styles.visualField} key={field.path} role="group">
                  <div className={styles.visualFieldMeta}>
                    <VisualOptionLabel
                      id={fieldId}
                      label={field.label}
                      description={field.description}
                      required={required}
                    />
                    {!required && !hasEnumValues && field.value_kind === "string" && (
                      <Toggle
                        className={styles.visualInclude}
                        aria-label={`Include ${field.label}`}
                        checked={included}
                        onCheckedChange={(checked) =>
                          onChange(field.path, {
                            included: checked,
                            ...(field.request_proxy_route && !checked
                              ? { proxy_routed: false, value: split.upstream }
                              : {}),
                          })
                        }
                      >
                        Include
                      </Toggle>
                    )}
                  </div>
                  <div className={styles.visualFieldControl}>
                    {field.value_kind === "bool" ? (
                      <NativeSelect
                        id={fieldId}
                        aria-label={`${field.label} value`}
                        required={required}
                        aria-required={required}
                        value={!included ? "__default" : String(Boolean(value))}
                        onChange={(event) => {
                          if (event.target.value === "__default") {
                            onChange(field.path, { included: false, value: undefined });
                            return;
                          }
                          onChange(field.path, {
                            included: true,
                            value: event.target.value === "true",
                          });
                        }}
                      >
                        {!required && <option value="__default">Default</option>}
                        <option value="true">Enabled</option>
                        <option value="false">Disabled</option>
                      </NativeSelect>
                    ) : hasEnumValues ? (
                      <NativeSelect
                        id={fieldId}
                        aria-label={`${field.label} value`}
                        required={required}
                        aria-required={required}
                        value={!included ? "__default" : String(value)}
                        onChange={(event) => {
                          if (event.target.value === "__default") {
                            onChange(field.path, { included: false, value: undefined });
                            return;
                          }
                          onChange(field.path, { included: true, value: event.target.value });
                        }}
                      >
                        {!required && <option value="__default">Default</option>}
                        {unsupportedValue !== null && (
                          <option value={unsupportedValue}>Unsupported: {unsupportedValue}</option>
                        )}
                        {field.enum_values.map((enumValue) => (
                          <option key={enumValue} value={enumValue}>
                            {enumValue}
                          </option>
                        ))}
                      </NativeSelect>
                    ) : (
                      <div className={styles.visualTextControl}>
                        <TextInput
                          id={fieldId}
                          type={field.sensitive && !isRevealed ? "password" : "text"}
                          disabled={!included}
                          value={String(value)}
                          required={required}
                          aria-required={required}
                          onChange={(event) => {
                            const nextValue = event.target.value;
                            onChange(field.path, {
                              value: routed && proxyRoute ? `${proxyRoute}${nextValue}` : nextValue,
                              ...(field.request_proxy_route ? { proxy_routed: routed } : {}),
                            });
                          }}
                          aria-label={field.label}
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
                        {field.request_proxy_route && (
                          <Toggle
                            className={styles.proxyToggle}
                            aria-label={`Route ${field.label} through Request Proxy`}
                            checked={routed}
                            disabled={!included || !proxyRoute || !proxyValueIsValid(String(value))}
                            onCheckedChange={(checked) => {
                              if (!checked) {
                                onChange(field.path, {
                                  value: String(value),
                                  proxy_routed: false,
                                });
                                return;
                              }
                              if (!proxyValueIsValid(String(value))) return;
                              onChange(field.path, {
                                value: `${proxyRoute}${String(value)}`,
                                proxy_routed: true,
                              });
                            }}
                          >
                            Proxy
                          </Toggle>
                        )}
                      </div>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        </section>
      ))}
      {provider && onProviderChange && (
        <section className={styles.visualGroup}>
          <header>
            <h3>Custom provider</h3>
          </header>
          <div className={styles.providerEditor}>
            <div className={styles.visualField}>
              <div className={styles.visualFieldMeta}>
                <VisualOptionLabel
                  id="config-option-custom-provider"
                  label="Use Custom provider"
                  description="Use the fixed custom provider instead of Codex's official OpenAI default."
                  required={false}
                />
              </div>
              <Toggle
                id="config-option-custom-provider"
                className={`${styles.visualInclude} ${styles.visualFieldControl}`}
                aria-label="Use Custom provider"
                checked={provider.included}
                onCheckedChange={(checked) =>
                  onProviderChange({
                    included: checked,
                    ...(checked
                      ? {
                          name: provider.name || "custom",
                          base_url: provider.base_url || "https://example.com/v1",
                        }
                      : {}),
                  })
                }
              >
                {provider.included ? "Enabled" : "Disabled"}
              </Toggle>
            </div>
            {provider.included && (
              <>
                <div className={styles.visualField}>
                  <div className={styles.visualFieldMeta}>
                    <VisualOptionLabel
                      id="config-option-custom-provider-name"
                      label="Name"
                      description="Display name for the fixed custom provider."
                      required
                    />
                  </div>
                  <div className={styles.visualFieldControl}>
                    <TextInput
                      id="config-option-custom-provider-name"
                      value={provider.name}
                      onChange={(event) => onProviderChange({ name: event.target.value })}
                      aria-label="Custom provider name"
                      required
                      aria-required="true"
                    />
                  </div>
                </div>
                <div className={styles.visualField}>
                  <div className={styles.visualFieldMeta}>
                    <VisualOptionLabel
                      id="config-option-custom-provider-base-url"
                      label="Base URL"
                      description="API base URL for the fixed custom provider."
                      required
                    />
                  </div>
                  <div className={`${styles.visualFieldControl} ${styles.visualTextControl}`}>
                    <TextInput
                      id="config-option-custom-provider-base-url"
                      value={(() => {
                        const route = requestProxyRoute(
                          tenant ?? { kind: "managed", name: "default" },
                          listen,
                        );
                        return splitRequestProxyValue(provider.base_url, route).upstream;
                      })()}
                      onChange={(event) => {
                        const route = requestProxyRoute(
                          tenant ?? { kind: "managed", name: "default" },
                          listen,
                        );
                        const routed = provider.proxy_routed && route;
                        onProviderChange({
                          base_url: routed ? `${route}${event.target.value}` : event.target.value,
                          proxy_routed: Boolean(routed),
                        });
                      }}
                      aria-label="Custom provider base URL"
                      required
                      aria-required="true"
                    />
                    {(() => {
                      const route = requestProxyRoute(
                        tenant ?? { kind: "managed", name: "default" },
                        listen,
                      );
                      const upstream = splitRequestProxyValue(provider.base_url, route).upstream;
                      const routed =
                        Boolean(provider.proxy_routed) ||
                        splitRequestProxyValue(provider.base_url, route).routed;
                      return (
                        <Toggle
                          className={styles.proxyToggle}
                          aria-label="Route Custom provider through Request Proxy"
                          checked={routed}
                          disabled={!route || !proxyValueIsValid(upstream)}
                          onCheckedChange={(checked) =>
                            onProviderChange({
                              base_url: checked ? `${route}${upstream}` : upstream,
                              proxy_routed: checked,
                            })
                          }
                        >
                          Proxy
                        </Toggle>
                      );
                    })()}
                  </div>
                </div>
              </>
            )}
          </div>
        </section>
      )}
    </div>
  );
}
function ConfigFilePane({
  api,
  tenant,
  agent,
  selection,
  file,
  mode,
  operationBusy,
  onControllerChange,
  onError,
  onRevealRetryChange,
  onSaved,
  onBeforeSave,
  onLinkedFileSaved,
  onVisualAvailable,
  onRequestRaw,
}: {
  api: ConfigApi;
  tenant: TenantSelection;
  agent: CodingAgentKind;
  selection: ConfigSelection;
  file: string;
  mode: "visual" | "raw";
  operationBusy: boolean;
  onControllerChange: (file: string, controller: ConfigFileController | null) => void;
  onError: (message: string | null) => void;
  onRevealRetryChange: (file: string, retry: (() => void) | null) => void;
  onSaved: () => void;
  onBeforeSave?: (customProvider: boolean) => boolean;
  onLinkedFileSaved?: (file: string) => void;
  onVisualAvailable?: (available: boolean) => void;
  onRequestRaw: () => void;
}) {
  const [snapshot, setSnapshot] = useState<ConfigFileData | null>(null);
  const [editor, setEditor] = useState("");
  const [visualOptions, setVisualOptions] = useState<ConfigVisualOption[] | null>(null);
  const [customProvider, setCustomProvider] = useState<ConfigCustomProvider | null>(null);
  const [textEditable, setTextEditable] = useState(true);
  const [rawDiagnostics, setRawDiagnostics] = useState<
    Array<{
      message: string;
      line: number;
      column: number;
    }>
  >([]);
  const [authMode, setAuthMode] = useState<ConfigAuthData["mode"]>("api-key");
  const [authKey, setAuthKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [feedback, setFeedback] = useState<"idle" | "saving" | "saved">("idle");
  const [revealed, setRevealed] = useState(false);
  const [reloadNonce, setReloadNonce] = useState(0);
  const rawEditorParent = useRef<HTMLDivElement | null>(null);
  const rawEditorView = useRef<EditorView | null>(null);
  const diagnoseTimer = useRef<number | null>(null);
  const diagnoseGeneration = useRef(0);
  const loadGeneration = useRef(0);
  const useCodeMirror = typeof navigator === "undefined" || !/jsdom/i.test(navigator.userAgent);
  const isAuth = file === "auth.json";
  const currentSelection = selection.current;
  const diagnose = useCallback(
    (value: string) => {
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const generation = ++diagnoseGeneration.current;
      diagnoseTimer.current = window.setTimeout(() => {
        void api
          .diagnoseConfigFile(
            {
              tenant,
              agent,
              current: currentSelection,
              config: currentSelection ? null : selection.config,
              file,
            },
            encodeBase64(new TextEncoder().encode(value)),
          )
          .then((result) => {
            if (generation === diagnoseGeneration.current)
              setRawDiagnostics(Array.isArray(result.diagnostics) ? result.diagnostics : []);
          })
          .catch(() => {
            if (generation === diagnoseGeneration.current) setRawDiagnostics([]);
          });
      }, 250);
    },
    [agent, api, currentSelection, file, tenant, selection.config],
  );
  const setFromSnapshot = useCallback(
    (value: ConfigFileData) => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const bytes = decodeBase64(value.content_base64);
      try {
        const content = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
        setEditor(content);
        setTextEditable(true);
        setRawDiagnostics([]);
        setVisualOptions(value.visual_options ?? null);
        setCustomProvider(() => {
          if (!value.custom_provider) return null;
          const route = requestProxyRoute(tenant, api.bootstrap?.listen);
          const split = splitRequestProxyValue(value.custom_provider.base_url, route);
          return {
            ...value.custom_provider,
            base_url: value.custom_provider.base_url,
            proxy_routed: split.routed,
          };
        });
        if (isAuth && value.auth) {
          setAuthMode(value.auth.mode);
          setAuthKey(value.auth.api_key ?? "");
        }
      } catch {
        setEditor("");
        setTextEditable(false);
        setVisualOptions(null);
        setRawDiagnostics([]);
      }
    },
    [api.bootstrap?.listen, isAuth, tenant],
  );
  useEffect(() => {
    const generation = ++loadGeneration.current;
    // A new file identity starts a fresh lifecycle before its external snapshot is loaded.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoading(true);
    setSnapshot(null);
    setEditor("");
    setVisualOptions(null);
    setCustomProvider(null);
    setRawDiagnostics([]);
    void api
      .revealConfigFile({
        tenant,
        agent,
        current: currentSelection,
        config: currentSelection ? null : selection.config,
        file,
      })
      .then((value) => {
        if (loadGeneration.current !== generation) return;
        onRevealRetryChange(file, null);
        setFromSnapshot(value);
        setSnapshot(value);
        if (!isAuth) onVisualAvailable?.(Boolean(value.visual_options && !value.visual_error));
      })
      .catch((cause) => {
        if (loadGeneration.current !== generation) return;
        onRevealRetryChange(file, () => setReloadNonce((value) => value + 1));
        onError(messageOf(cause));
      })
      .finally(() => {
        if (loadGeneration.current === generation) setLoading(false);
      });
    return () => {
      loadGeneration.current += 1;
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      onRevealRetryChange(file, null);
    };
  }, [
    agent,
    api,
    currentSelection,
    file,
    isAuth,
    onError,
    onRevealRetryChange,
    onVisualAvailable,
    tenant,
    selection.config,
    setFromSnapshot,
    reloadNonce,
  ]);
  useEffect(() => {
    if (mode === "raw" && snapshot && textEditable) diagnose(editor);
  }, [diagnose, editor, mode, snapshot, textEditable]);
  const editorBytes = useMemo(() => {
    if (!snapshot || !textEditable) return null;
    return new TextEncoder().encode(editor);
  }, [editor, snapshot, textEditable]);
  const visualDirty =
    Boolean(snapshot && visualOptions) &&
    JSON.stringify(
      visualOptions?.map(({ path, included, value }) => ({ path, included, value })),
    ) !==
      JSON.stringify(
        snapshot?.visual_options?.map(({ path, included, value }) => ({ path, included, value })),
      );
  const providerDirty = Boolean(
    snapshot &&
    customProvider &&
    JSON.stringify(comparableProvider(customProvider)) !==
      JSON.stringify(comparableProvider(snapshot.custom_provider)),
  );
  const authDirty =
    isAuth &&
    Boolean(snapshot?.auth) &&
    (authMode !== snapshot?.auth?.mode || authKey !== (snapshot?.auth?.api_key ?? ""));
  const dirty =
    mode === "visual"
      ? isAuth
        ? authDirty
        : visualDirty || providerDirty
      : editorBytes !== null &&
        snapshot !== null &&
        encodeBase64(editorBytes) !== snapshot.content_base64;
  const canSave = Boolean(
    snapshot && textEditable && (isAuth ? authMode === "api-key" || mode === "raw" : true),
  );
  const save = useCallback(async (): Promise<boolean> => {
    if (operationBusy || !snapshot || !editorBytes || !canSave) return false;
    if (
      mode === "visual" &&
      !isAuth &&
      customProvider?.included &&
      onBeforeSave &&
      !onBeforeSave(true)
    )
      return false;
    if (mode === "visual" && !isAuth && visualOptions) {
      for (const field of visualOptions) {
        if (!field.included || !field.request_proxy_route || typeof field.value !== "string")
          continue;
        const route = requestProxyRoute(tenant, api.bootstrap?.listen);
        const split = splitRequestProxyValue(field.value, route);
        if (!proxyValueIsValid(split.upstream)) {
          onError(`${field.label} must contain a valid HTTP or HTTPS upstream URL.`);
          return false;
        }
      }
    }
    if (mode === "visual" && isAuth && snapshot.auth?.extra_fields) {
      if (!window.confirm("Replace the extra native credential fields with an API-key object?"))
        return false;
    }
    if (mode === "visual" && !isAuth && customProvider?.included) {
      if (!customProvider.name.trim() || !customProvider.base_url.trim()) {
        onError("Custom provider name and base URL must not be empty.");
        return false;
      }
      const route = requestProxyRoute(tenant, api.bootstrap?.listen);
      const upstream = splitRequestProxyValue(customProvider.base_url, route).upstream;
      if (!proxyValueIsValid(upstream)) {
        onError("Custom provider base URL must contain a valid HTTP or HTTPS URL.");
        return false;
      }
    }
    setFeedback("saving");
    try {
      const value = await api.saveConfigFile(
        {
          tenant,
          agent,
          current: currentSelection,
          config: currentSelection ? null : selection.config,
          file,
        },
        {
          revision: snapshot.revision,
          contentBase64: encodeBase64(editorBytes),
          ...(mode === "visual" && !isAuth && visualOptions
            ? {
                visualOptions: visualOptions.map(({ path, included, value: fieldValue }) => ({
                  path,
                  included,
                  value: fieldValue,
                })),
              }
            : {}),
          ...(mode === "visual" && !isAuth && customProvider
            ? {
                customProvider: {
                  included: customProvider.included,
                  name: customProvider.name,
                  base_url: customProvider.base_url,
                  proxy_routed: Boolean(customProvider.proxy_routed),
                },
              }
            : {}),
          ...(mode === "visual" && isAuth
            ? { visualAuth: { included: true, value: authKey } }
            : {}),
        },
      );
      setFromSnapshot(value);
      setSnapshot(value);
      if (value.linked_file) onLinkedFileSaved?.(value.linked_file.file);
      setFeedback("saved");
      onError(null);
      onSaved();
      window.setTimeout(() => setFeedback("idle"), 4000);
      return true;
    } catch (cause) {
      setFeedback("idle");
      onError(messageOf(cause));
      return false;
    }
  }, [
    agent,
    api,
    authKey,
    canSave,
    editorBytes,
    file,
    isAuth,
    mode,
    onError,
    onSaved,
    operationBusy,
    tenant,
    currentSelection,
    selection.config,
    setFromSnapshot,
    snapshot,
    visualOptions,
    customProvider,
    onBeforeSave,
    onLinkedFileSaved,
  ]);
  const restore = useCallback(() => {
    if (!snapshot) return;
    setFromSnapshot(snapshot);
    onError(null);
  }, [onError, setFromSnapshot, snapshot]);
  useEffect(() => {
    onControllerChange(
      file,
      snapshot
        ? { dirty, canSave, save, restore, reload: () => setReloadNonce((value) => value + 1) }
        : null,
    );
    return () => onControllerChange(file, null);
  }, [canSave, dirty, file, onControllerChange, restore, save, snapshot]);
  useEffect(() => {
    if (!useCodeMirror || mode !== "raw" || !snapshot || !textEditable || !rawEditorParent.current)
      return;
    const language = file.endsWith(".json") ? json() : StreamLanguage.define(toml);
    const view = new EditorView({
      parent: rawEditorParent.current,
      doc: editor,
      extensions: [
        basicSetup,
        language,
        EditorView.cspNonce.of(codeMirrorCspNonce()),
        syntaxHighlighting(configHighlightStyle),
        lintGutter(),
        keymap.of([indentWithTab]),
        EditorView.contentAttributes.of({ "aria-label": `${file} content` }),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          const value = update.state.doc.toString();
          setEditor(value);
          diagnose(value);
        }),
      ],
    });
    rawEditorView.current = view;
    return () => {
      diagnoseGeneration.current += 1;
      view.destroy();
      rawEditorView.current = null;
    };
    // Keep the editor instance alive while its document changes; the next effect synchronizes it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [diagnose, file, mode, snapshot, textEditable, useCodeMirror]);
  useEffect(() => {
    const view = rawEditorView.current;
    if (!view || mode !== "raw") return;
    if (view.state.doc.toString() !== editor)
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: editor } });
  }, [editor, mode]);
  useEffect(() => {
    const view = rawEditorView.current;
    if (!view || mode !== "raw") return;
    view.dispatch(
      setDiagnostics(
        view.state,
        rawDiagnostics.map((diagnostic) => {
          const lineInfo = view.state.doc.line(
            Math.min(Math.max(1, diagnostic.line), view.state.doc.lines),
          );
          const from = Math.min(lineInfo.from + Math.max(1, diagnostic.column) - 1, lineInfo.to);
          return {
            from,
            to: Math.min(from + 1, lineInfo.to),
            severity: "error" as const,
            message: diagnostic.message,
          };
        }),
      ),
    );
  }, [mode, rawDiagnostics]);
  const updateVisualOption = useCallback((path: string, update: Partial<ConfigVisualOption>) => {
    setVisualOptions((fields) => {
      if (!fields) return null;
      const next = fields.map((field) => (field.path === path ? { ...field, ...update } : field));
      return next;
    });
  }, []);
  const updateCustomProvider = useCallback((update: Partial<ConfigCustomProvider>) => {
    setCustomProvider((provider) => (provider ? { ...provider, ...update } : provider));
  }, []);
  if (loading)
    return (
      <div className={styles.configFilePane}>
        <Loading />
      </div>
    );
  if (!snapshot) return <div className={styles.configFilePane} />;
  return (
    <section className={styles.configFilePane} aria-label={`${file} editor`}>
      <div className={styles.editorTools}>
        <div className={styles.fileTitle}>
          <strong>{file}</strong>
          <span>{snapshot.exists ? "Existing file" : "New file"}</span>
        </div>
        {isAuth && mode === "visual" && <span className={styles.authModeBadge}>{authMode}</span>}
        <ActionButton
          tone="primary"
          disabled={operationBusy || !dirty || !canSave}
          onClick={() => void save()}
        >
          {feedback === "saving" ? <LoaderCircle className="spin" size={14} /> : <Save size={14} />}
          <span aria-live="polite">
            {feedback === "saving" ? "Saving…" : feedback === "saved" ? "Saved" : "Save"}
          </span>
        </ActionButton>
      </div>
      {snapshot.warnings && snapshot.warnings.length > 0 && (
        <div className={styles.fileWarnings} role="status">
          {snapshot.warnings.map((warning) => (
            <span key={warning}>
              <AlertTriangle size={14} /> {warning}
            </span>
          ))}
        </div>
      )}
      {mode === "visual" && !isAuth && visualOptions ? (
        <VisualConfigOptions
          fields={visualOptions}
          provider={customProvider ?? undefined}
          onChange={updateVisualOption}
          onProviderChange={updateCustomProvider}
          tenant={tenant}
          listen={api.bootstrap?.listen}
        />
      ) : mode === "visual" && isAuth && snapshot.auth ? (
        <div className={styles.visualEditor}>
          <section className={styles.visualGroup}>
            <header>
              <h3>Credentials</h3>
            </header>
            <div className={styles.authVisualBody}>
              {authMode === "chatgpt" ? (
                <>
                  <div className={styles.authStatus} role="status">
                    <Check size={16} /> ChatGPT credentials are active.
                  </div>
                  <p>Use Raw to inspect the native token object, or switch to an API key.</p>
                  <div className={styles.dialogActions}>
                    <button type="button" onClick={onRequestRaw}>
                      Open Raw
                    </button>
                    <ActionButton
                      tone="primary"
                      onClick={() => {
                        if (!window.confirm("Switch this draft to API-key credentials?")) return;
                        setAuthMode("api-key");
                      }}
                    >
                      Switch to API key credentials
                    </ActionButton>
                  </div>
                </>
              ) : (
                <div className={styles.visualField}>
                  <div className={styles.visualFieldMeta}>
                    <VisualOptionLabel
                      id="config-option-openai-api-key"
                      label="OpenAI API key"
                      description="API key used by Codex for OpenAI authentication."
                      required={false}
                    />
                  </div>
                  <div className={`${styles.visualFieldControl} ${styles.visualTextControl}`}>
                    <TextInput
                      id="config-option-openai-api-key"
                      type={revealed ? "text" : "password"}
                      value={authKey}
                      onChange={(event) => setAuthKey(event.target.value)}
                      aria-label="OpenAI API key"
                    />
                    <IconButton
                      label={revealed ? "Hide OpenAI API key" : "Show OpenAI API key"}
                      onClick={() => setRevealed((value) => !value)}
                    >
                      {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
                    </IconButton>
                  </div>
                </div>
              )}
              {snapshot.auth.warnings.map((warning) => (
                <div className={styles.inlineWarning} key={warning}>
                  <AlertTriangle size={15} /> <span>{warning}</span>
                </div>
              ))}
              {authMode === "api-key" && snapshot.auth.extra_fields && (
                <div className={styles.inlineWarning}>
                  <AlertTriangle size={15} />
                  <span>Saving will replace extra native credential fields.</span>
                </div>
              )}
            </div>
          </section>
        </div>
      ) : textEditable ? (
        useCodeMirror ? (
          <div ref={rawEditorParent} className={styles.codeEditor} aria-label={`${file} content`} />
        ) : (
          <TextArea
            className={`${styles.codeEditor} ${styles.codeEditorFallback}`}
            aria-label={`${file} content`}
            value={editor}
            onChange={(event) => {
              setEditor(event.target.value);
              diagnose(event.target.value);
            }}
            spellCheck={false}
          />
        )
      ) : (
        <div className={styles.binaryConfigNotice} role="status">
          <AlertTriangle size={18} />
          <span>This file is not valid UTF-8 and cannot be edited in the Console.</span>
          <button
            type="button"
            onClick={() => {
              const raw = decodeBase64(snapshot.content_base64);
              const url = URL.createObjectURL(new Blob([new Uint8Array(raw).buffer]));
              const link = document.createElement("a");
              link.href = url;
              link.download = file;
              link.click();
              URL.revokeObjectURL(url);
            }}
          >
            <Download size={14} /> Download raw file
          </button>
        </div>
      )}
      {mode === "raw" && rawDiagnostics.length > 0 && (
        <div className={styles.editorDiagnostics} role="alert">
          {rawDiagnostics.map((diagnostic, index) => (
            <span key={`${diagnostic.line}-${diagnostic.column}-${index}`}>
              Line {diagnostic.line}, column {diagnostic.column}: {diagnostic.message}
            </span>
          ))}
        </div>
      )}
    </section>
  );
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
    changePageLocation("configs", configLocation(tenant, agent, null), onLocationChange, true);
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
        icon: <AgentIcon agent={value} size={14} />,
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
          changePageLocation(
            "configs",
            configLocation(tenant, agent, null),
            onLocationChange,
            true,
          );
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
      changePageLocation(
        "configs",
        configLocation(tenantSelectionFromConfigKey(next), agent, null),
        onLocationChange,
      );
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
      changePageLocation("configs", configLocation(tenant, next, null), onLocationChange);
    });
  }
  function openConfig(name: string) {
    requestEditorAction(() => {
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      const nextSelection: ConfigSelection = { current: false, config: name };
      changePageLocation(
        "configs",
        configLocation(tenant, agent, nextSelection, file),
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
        configLocation(tenant, agent, { current: true }, file),
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
      await api.createConfig(tenant, agent, name);
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadCatalog("background");
      setSelection({ current: false, config: name });
      setDetailOpen(true);
      changePageLocation(
        "configs",
        configLocation(tenant, agent, { current: false, config: name }, file),
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
        changePageLocation("configs", configLocation(tenant, agent, null), onLocationChange, true);
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
          changePageLocation(
            "configs",
            configLocation(tenant, agent, null),
            onLocationChange,
            true,
          );
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
    <div className={`${styles.page} ${styles.configPage}`}>
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
                  <SelectionMenu
                    className={styles.sessionTenantFilter}
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
              {!managedTenantMissing && (
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
                const issue = configIssuePresentation(entry) ?? configWarningPresentation(entry);
                const issueDescriptionId = issue
                  ? configIssueDescriptionId(tenant, agent, entry.name)
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
                      changePageLocation(
                        "configs",
                        configLocation(tenant, agent, null),
                        onLocationChange,
                      );
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
          className={styles.dialog}
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
