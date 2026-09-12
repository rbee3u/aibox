import { FieldDifferences } from "@/features/configs/detail/ConfigDifferences";
import { Eye, EyeOff } from "lucide-react";
import { useState } from "react";
import type { ConfigCustomProvider, ConfigVisualOption } from "@/api/configs";
import type { TenantSelection } from "@/domain/tenant";
import {
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import { IconButton } from "@/shared/ui/IconButton";
import { TextInput, Toggle } from "@/shared/ui/FormControls";
import {
  customProviderPaths,
  visualOptionAvailable,
} from "@/features/configs/detail/configFileModel";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import styles from "@/features/configs/ConfigPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const BLANK_SELECT = "__blank__";

/**
 * A field's label over its native path, so the form, the difference list, and
 * the Raw file all name a field the same way.
 */
export function VisualOptionLabel({ label, path }: { label: string; path?: string }) {
  return (
    <div className={styles.visualOptionLabel}>
      <span>{label}</span>
      {path && <code className={styles.visualOptionPath}>{path}</code>}
    </div>
  );
}

function VisualFieldMeta({
  label,
  path,
  required,
  include,
}: {
  label: string;
  path?: string;
  required: boolean;
  include?: {
    id?: string;
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
  };
}) {
  return (
    <div className={styles.visualFieldMeta}>
      <VisualOptionLabel label={label} path={path} />
      {required ? (
        <>
          <span className={styles.requiredMarker} aria-hidden="true">
            *
          </span>
          <span className={styles.visualPresenceLabel}>Required</span>
        </>
      ) : include ? (
        <Toggle
          id={include.id}
          className={styles.visualInclude}
          aria-label={`Optional ${label}`}
          checked={include.checked}
          onCheckedChange={include.onCheckedChange}
        >
          Optional
        </Toggle>
      ) : null}
    </div>
  );
}

function hasStoredValue(field: ConfigVisualOption): boolean {
  if (field.value_kind === "bool") return typeof field.value === "boolean";
  if (typeof field.value !== "string") return false;
  return field.enum_values.length === 0 || field.value.length > 0;
}

function includeUpdate(
  field: ConfigVisualOption,
  checked: boolean,
  extras: Partial<ConfigVisualOption> = {},
): Partial<ConfigVisualOption> {
  if (!checked) return { included: false, ...extras };
  if (hasStoredValue(field)) return { included: true, ...extras };
  if (field.enum_values.length > 0) {
    const first = field.enum_values[0];
    if (first !== undefined) return { included: true, value: first, ...extras };
  }
  if (field.value_kind === "bool") return { included: true, value: false, ...extras };
  return { included: true, ...extras };
}

/** The prefix a routed value carries, stated so the toggle's effect is visible. */
function ProxyRouteNote({ route }: { route: string }) {
  return (
    <small className={styles.proxyRouteNote}>
      Routed through the Request Proxy at <code>{route}</code>
    </small>
  );
}

export function VisualConfigOptions({
  file,
  fields,
  provider,
  invalidPaths,
  onChange,
  onProviderChange,
  tenant,
  listen,
}: {
  file: string;
  fields: ConfigVisualOption[];
  provider?: ConfigCustomProvider;
  /** Native paths a refused save named; their inputs read as invalid until edited. */
  invalidPaths?: readonly string[];
  onChange: (path: string, update: Partial<ConfigVisualOption>) => void;
  onProviderChange?: (update: Partial<ConfigCustomProvider>) => void;
  tenant?: TenantSelection;
  listen?: string;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const customProviderSelected = Boolean(provider?.included);
  const invalid = (path: string) => (invalidPaths?.includes(path) ? true : undefined);
  const proxyRouteFor = () =>
    requestProxyRoute(tenant ?? { kind: "managed", name: "default" }, listen);
  return (
    <div className={styles.visualEditor}>
      <div className={styles.visualFieldList}>
        {provider && onProviderChange && (
          <div className={styles.providerEditor}>
            <div className={styles.visualField}>
              <VisualFieldMeta
                label="Custom provider"
                path="model_providers.custom"
                required={false}
                include={{
                  id: "config-option-custom-provider",
                  checked: provider.included,
                  onCheckedChange: (checked) =>
                    onProviderChange({
                      included: checked,
                      ...(checked
                        ? {
                            name: provider.name || "custom",
                            base_url: provider.base_url || "https://example.com/v1",
                          }
                        : {}),
                    }),
                }}
              />
              <div className={styles.visualFieldControl}>
                {provider.included && (
                  <TextInput
                    id="config-option-custom-provider-name"
                    value={provider.name}
                    onChange={(event) => onProviderChange({ name: event.target.value })}
                    aria-label="Custom provider name"
                    aria-invalid={invalid(customProviderPaths.name)}
                    required
                    aria-required="true"
                  />
                )}
              </div>
            </div>
            <FieldDifferences file={file} path="model_provider" />
            <FieldDifferences file={file} path="model_providers.custom" />
            {provider.included && (
              <CustomProviderBaseUrlField
                provider={provider}
                route={proxyRouteFor()}
                invalid={invalid(customProviderPaths.baseUrl)}
                onProviderChange={onProviderChange}
              />
            )}
          </div>
        )}
        {fields
          .filter((field) => visualOptionAvailable(field, fields))
          .map((field) => {
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
            const selectValue =
              field.value_kind === "bool"
                ? typeof field.value === "boolean"
                  ? String(field.value)
                  : ""
                : typeof value === "string"
                  ? value
                  : "";
            return (
              <article className={styles.visualField} key={field.path} role="group">
                <VisualFieldMeta
                  label={field.label}
                  path={field.path}
                  required={required}
                  include={
                    required
                      ? undefined
                      : {
                          checked: included,
                          onCheckedChange: (checked) =>
                            onChange(
                              field.path,
                              includeUpdate(
                                field,
                                checked,
                                field.request_proxy_route && !checked
                                  ? { proxy_routed: false, value: split.upstream }
                                  : {},
                              ),
                            ),
                        }
                  }
                />
                <div className={styles.visualFieldControl}>
                  {field.value_kind === "bool" ? (
                    <VisualFieldSelect
                      id={fieldId}
                      label={field.label}
                      required={required}
                      disabled={!included}
                      value={selectValue}
                      options={[
                        { value: "true", label: "Enabled" },
                        { value: "false", label: "Disabled" },
                      ]}
                      onSelect={(next) => {
                        onChange(field.path, {
                          included: true,
                          value: next === "true",
                        });
                      }}
                    />
                  ) : hasEnumValues ? (
                    <VisualFieldSelect
                      id={fieldId}
                      label={field.label}
                      required={required}
                      disabled={!included}
                      value={selectValue}
                      options={[
                        ...(unsupportedValue !== null
                          ? [
                              {
                                value: unsupportedValue,
                                label: `Unsupported: ${unsupportedValue}`,
                              },
                            ]
                          : []),
                        ...field.enum_values.map((enumValue) => ({
                          value: enumValue,
                          label: enumValue,
                        })),
                      ]}
                      onSelect={(next) => {
                        onChange(field.path, { included: true, value: next });
                      }}
                    />
                  ) : (
                    <>
                      <div className={styles.visualTextControl}>
                        <TextInput
                          id={fieldId}
                          type={field.sensitive && !isRevealed ? "password" : "text"}
                          disabled={!included}
                          value={String(value)}
                          required={required}
                          aria-required={required}
                          aria-invalid={invalid(field.path)}
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
                            {isRevealed ? (
                              <EyeOff size={iconSize.xs} />
                            ) : (
                              <Eye size={iconSize.xs} />
                            )}
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
                      {routed && proxyRoute && <ProxyRouteNote route={proxyRoute} />}
                    </>
                  )}
                </div>
                <FieldDifferences file={file} path={field.path} sensitive={field.sensitive} />
              </article>
            );
          })}
      </div>
    </div>
  );
}

function CustomProviderBaseUrlField({
  provider,
  route,
  invalid,
  onProviderChange,
}: {
  provider: ConfigCustomProvider;
  route: string | null;
  invalid: true | undefined;
  onProviderChange: (update: Partial<ConfigCustomProvider>) => void;
}) {
  const { upstream, routed: valueRouted } = splitRequestProxyValue(provider.base_url, route);
  const routed = Boolean(provider.proxy_routed) || valueRouted;
  return (
    <div className={styles.visualField}>
      <VisualFieldMeta label="Base URL" path={customProviderPaths.baseUrl} required />
      <div className={styles.visualFieldControl}>
        <div className={styles.visualTextControl}>
          <TextInput
            id="config-option-custom-provider-base-url"
            value={upstream}
            onChange={(event) => {
              const keepRouted = provider.proxy_routed && route;
              onProviderChange({
                base_url: keepRouted ? `${route}${event.target.value}` : event.target.value,
                proxy_routed: Boolean(keepRouted),
              });
            }}
            aria-label="Custom provider base URL"
            aria-invalid={invalid}
            required
            aria-required="true"
          />
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
        </div>
        {routed && route && <ProxyRouteNote route={route} />}
      </div>
    </div>
  );
}

function VisualFieldSelect({
  id,
  label,
  required,
  disabled,
  value,
  options,
  onSelect,
}: {
  id: string;
  label: string;
  required: boolean;
  disabled: boolean;
  value: string;
  options: ReadonlyArray<{ value: string; label: string }>;
  onSelect: (value: string) => void;
}) {
  const blank = value === "";
  return (
    <SelectionMenu
      className={styles.visualFieldSelect}
      variant="field"
      id={id}
      required={required}
      disabled={disabled}
      label={label}
      pluralLabel="values"
      selected={new Set([blank ? BLANK_SELECT : value])}
      unavailableSummary={blank ? "" : value}
      options={options}
      onCommit={(values) => {
        const next = [...values][0];
        if (next !== undefined && next !== BLANK_SELECT) onSelect(next);
      }}
    />
  );
}
