import { Eye, EyeOff } from "lucide-react";
import { useMemo, useState } from "react";
import type { ConfigCustomProvider, ConfigVisualOption } from "@/api/configs";
import type { TenantSelection } from "@/api/tenantSelection";
import {
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import { IconButton } from "@/shared/ui/IconButton";
import { NativeSelect, TextInput, Toggle } from "@/shared/ui/FormControls";
import { HelpTooltip } from "@/shared/ui/IssueIndicator";
import styles from "@/features/configs/ConfigPage.module.css";

export function VisualOptionLabel({
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
export function VisualConfigOptions({
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
