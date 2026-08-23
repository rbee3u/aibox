import type { Page } from "@playwright/test";
import type { ConfigVisualOption } from "../src/controlApi";

type VisualOptionFixture = Omit<
  ConfigVisualOption,
  "required" | "request_proxy_route" | "proxy_routed"
> &
  Partial<Pick<ConfigVisualOption, "required" | "request_proxy_route" | "proxy_routed">>;

export const rawConfig = [
  "# Raw editor syntax",
  'approval_policy = "never"',
  "retry_count = 3",
  "enabled = true",
  "",
  "[model_providers.custom]",
  'name = "custom"',
  "",
].join("\n");

export async function mockConfigs(page: Page) {
  await page.route("**/_aibox/api/**", (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (path === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (path === "/_aibox/api/configs" && request.method() === "GET") {
      return route.fulfill({
        json: {
          configs: [],
          files: ["config.toml"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        },
      });
    }
    if (path === "/_aibox/api/configs/reveal") {
      return route.fulfill({
        json: {
          file: "config.toml",
          exists: true,
          revision: "config-revision",
          content_base64: btoa(rawConfig),
        },
      });
    }
    if (path === "/_aibox/api/configs/diagnose") {
      return route.fulfill({ json: { diagnostics: [] } });
    }
    throw new Error("Unexpected Configs API request: " + request.method() + " " + path);
  });
}

const codexVisualFields = [
  {
    path: "approval_policy",
    label: "Approval policy",
    description: "Controls when Codex pauses before executing commands.",
    group: "Execution & permissions",
    value_kind: "string",
    enum_values: ["untrusted", "on-request", "never"],
    sensitive: false,
    required: true,
    included: true,
    value: "never",
  },
  {
    path: "sandbox_mode",
    label: "Sandbox mode",
    description: "Filesystem and network access policy for command execution.",
    group: "Execution & permissions",
    value_kind: "string",
    enum_values: ["read-only", "workspace-write", "danger-full-access"],
    sensitive: false,
    required: true,
    included: true,
    value: "danger-full-access",
  },
  {
    path: "model_reasoning_effort",
    label: "Model reasoning effort",
    description: "Reasoning effort for supported models.",
    group: "Model & reasoning",
    value_kind: "string",
    enum_values: ["minimal", "low", "medium", "high", "xhigh"],
    sensitive: false,
    included: false,
  },
  {
    path: "plan_mode_reasoning_effort",
    label: "Plan mode reasoning effort",
    description: "Reasoning effort override used in Plan mode.",
    group: "Model & reasoning",
    value_kind: "string",
    enum_values: ["none", "minimal", "low", "medium", "high", "xhigh"],
    sensitive: false,
    included: true,
    value: "high",
  },
  {
    path: "model",
    label: "Model",
    description: "Model selected for Codex sessions.",
    group: "Model & reasoning",
    value_kind: "string",
    enum_values: [],
    sensitive: false,
    required: true,
    included: true,
    value: "gpt-5.6-sol",
  },
] satisfies VisualOptionFixture[];

export async function mockCodexVisual(page: Page) {
  const content = [
    'approval_policy = "never"',
    'sandbox_mode = "danger-full-access"',
    'plan_mode_reasoning_effort = "high"',
    'model = "gpt-5.6-sol"',
    "",
  ].join("\n");
  await page.route("**/_aibox/api/**", (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (path === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (path === "/_aibox/api/configs" && request.method() === "GET") {
      return route.fulfill({
        json: {
          configs: [{ name: "team", state: "ready" }],
          files: ["config.toml"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        },
      });
    }
    if (path === "/_aibox/api/configs/reveal") {
      return route.fulfill({
        json: {
          file: "config.toml",
          exists: true,
          revision: "config-revision",
          content_base64: btoa(content),
          visual_options: codexVisualFields,
          custom_provider: {
            included: false,
            name: "custom",
            base_url: "https://example.com/v1",
            request_proxy_route: true,
            proxy_routed: false,
          },
        },
      });
    }
    if (path === "/_aibox/api/configs/diagnose") {
      return route.fulfill({ json: { diagnostics: [] } });
    }
    throw new Error("Unexpected Codex Visual API request: " + request.method() + " " + path);
  });
}

const settingsContent = JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "https://api.example.test",
      ANTHROPIC_AUTH_TOKEN: "test-token-not-a-secret",
    },
    permissions: { defaultMode: "bypassPermissions" },
    skipDangerousModePermissionPrompt: true,
  },
  null,
  2,
);

const visualFields = [
  {
    path: "env.ANTHROPIC_BASE_URL",
    label: "Anthropic base URL",
    description: "Endpoint used by Claude.",
    group: "Endpoint & credentials",
    value_kind: "string",
    enum_values: [],
    sensitive: false,
    included: true,
    value: "https://api.example.test",
  },
  {
    path: "env.ANTHROPIC_AUTH_TOKEN",
    label: "Anthropic auth token",
    description: "Credential sent to the Anthropic endpoint.",
    group: "Endpoint & credentials",
    value_kind: "string",
    enum_values: [],
    sensitive: true,
    included: true,
    value: "test-token-not-a-secret",
  },
  {
    path: "permissions.defaultMode",
    label: "Default permission mode",
    description: "Native Claude permission mode.",
    group: "Permissions",
    value_kind: "string",
    enum_values: ["bypassPermissions"],
    sensitive: false,
    included: true,
    value: "bypassPermissions",
  },
  {
    path: "skipDangerousModePermissionPrompt",
    label: "Skip dangerous mode prompt",
    description: "Skip Claude's dangerous mode confirmation.",
    group: "Permissions",
    value_kind: "bool",
    enum_values: [],
    sensitive: false,
    included: true,
    value: true,
  },
] satisfies VisualOptionFixture[];

export async function mockConfigWorkflows(page: Page) {
  await page.route("**/_aibox/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (path === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (path === "/_aibox/api/configs" && request.method() === "GET") {
      return route.fulfill({
        json: {
          configs: [{ name: "team", state: "ready" }],
          files: ["settings.json"],
          application: {
            last_application: { applied: "team", applied_at: "2026-08-19T02:00:00Z" },
            drift: "dirty",
          },
          credential_propagation_available: false,
        },
      });
    }
    if (path === "/_aibox/api/configs/reveal") {
      return route.fulfill({
        json: {
          file: "settings.json",
          exists: true,
          revision: "settings-revision",
          content_base64: btoa(settingsContent),
          visual_options: visualFields,
        },
      });
    }
    if (path === "/_aibox/api/configs/diagnose") {
      return route.fulfill({ json: { diagnostics: [] } });
    }
    throw new Error("Unexpected Config workflow API request: " + request.method() + " " + path);
  });
}
