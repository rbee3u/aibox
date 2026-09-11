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
import { visualOptionAvailable } from "@/features/configs/detail/configFileModel";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import styles from "@/features/configs/ConfigPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const BLANK_SELECT = "__blank__";

export function VisualOptionLabel({ label }: { label: string }) {
  return (
    <div className={styles.visualOptionLabel}>
      <span>{label}</span>
    </div>
  );
}

function VisualFieldMeta({
  label,
  required,
  include,
}: {
  label: string;
  required: boolean;
  include?: {
    id?: string;
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
  };
}) {
  return (
    <div className={styles.visualFieldMeta}>
      <VisualOptionLabel label={label} />
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

export function VisualConfigOptions({
  file,
  fields,
  provider,
  onChange,
  onProviderChange,
  tenant,
  listen,
}: {
  file: string;
  fields: ConfigVisualOption[];
  provider?: ConfigCustomProvider;
  onChange: (path: string, update: Partial<ConfigVisualOption>) => void;
  onProviderChange?: (update: Partial<ConfigCustomProvider>) => void;
  tenant?: TenantSelection;
  listen?: string;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const customProviderSelected = Boolean(provider?.included);
  return (
    <div className={styles.visualEditor}>
      <div className={styles.visualFieldList}>
        {provider && onProviderChange && (
          <div className={styles.providerEditor}>
            <div className={styles.visualField}>
              <VisualFieldMeta
                label="Custom provider"
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
                    required
                    aria-required="true"
                  />
                )}
              </div>
            </div>
            <FieldDifferences file={file} path="model_provider" />
            <FieldDifferences file={file} path="model_providers.custom" />
            {provider.included && (
              <div className={styles.visualField}>
                <VisualFieldMeta label="Base URL" required />
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
                          {isRevealed ? <EyeOff size={iconSize.xs} /> : <Eye size={iconSize.xs} />}
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
                <FieldDifferences file={file} path={field.path} sensitive={field.sensitive} />
              </article>
            );
          })}
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
