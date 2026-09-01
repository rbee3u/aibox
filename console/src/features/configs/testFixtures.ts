import type { ConfigFileData, ConfigVisualOption } from "@/api/configs";

export type VisualOptionFixture = Omit<
  ConfigVisualOption,
  "required" | "request_proxy_route" | "proxy_routed"
> &
  Partial<Pick<ConfigVisualOption, "required" | "request_proxy_route" | "proxy_routed">>;

export function configFile(
  file: string,
  content: string,
  visualOptions?: VisualOptionFixture[],
  customProvider?: ConfigFileData["custom_provider"],
): ConfigFileData {
  return {
    file,
    exists: true,
    revision: `${file}-revision`,
    content_base64: btoa(content),
    ...(visualOptions
      ? {
          visual_options: visualOptions.map((option) => ({
            required: false,
            request_proxy_route: false,
            proxy_routed: false,
            ...option,
          })),
        }
      : {}),
    ...(customProvider ? { custom_provider: customProvider } : {}),
  };
}

export function claudeVisualOptions(): VisualOptionFixture[] {
  return [
    ["env.ANTHROPIC_BASE_URL", "Anthropic base URL", "string", "https://example.com"],
    ["env.ANTHROPIC_AUTH_TOKEN", "Anthropic auth token", "string", "secret"],
    ["env.ANTHROPIC_DEFAULT_HAIKU_MODEL", "Default Haiku model", "string", "haiku"],
    ["env.ANTHROPIC_DEFAULT_SONNET_MODEL", "Default Sonnet model", "string", "sonnet"],
    ["env.ANTHROPIC_DEFAULT_OPUS_MODEL", "Default Opus model", "string", "opus"],
    ["env.ANTHROPIC_DEFAULT_FABLE_MODEL", "Default Fable model", "string", "fable"],
    ["permissions.defaultMode", "Default permission mode", "string", "bypassPermissions"],
    ["skipDangerousModePermissionPrompt", "Skip dangerous mode prompt", "bool", true],
  ].map(([path, label, valueKind, value]) => ({
    path: path as string,
    label: label as string,
    description: `${label as string} description`,
    group:
      path === "permissions.defaultMode" || path === "skipDangerousModePermissionPrompt"
        ? "Permissions"
        : "Endpoint & credentials",
    value_kind: valueKind as "string" | "bool",
    enum_values: [],
    sensitive: path === "env.ANTHROPIC_AUTH_TOKEN",
    required:
      path === "env.ANTHROPIC_BASE_URL" ||
      path === "env.ANTHROPIC_AUTH_TOKEN" ||
      path === "permissions.defaultMode",
    included: true,
    value,
  }));
}
