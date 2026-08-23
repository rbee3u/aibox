import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigFileData, ConfigListData, ConfigVisualOption } from "./controlApi";
import { ConfigPage, tenants, activeOperation } from "./managementTestSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("ConfigPage", () => {
  type VisualOptionFixture = Omit<
    ConfigVisualOption,
    "required" | "request_proxy_route" | "proxy_routed"
  > &
    Partial<Pick<ConfigVisualOption, "required" | "request_proxy_route" | "proxy_routed">>;

  function configFile(
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
  function claudeVisualOptions(): VisualOptionFixture[] {
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
  it("replaces a failed Config catalog load with an error state and Retry", async () => {
    let catalogAttempts = 0;
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex") {
        catalogAttempts += 1;
        return catalogAttempts === 1
          ? Promise.reject(new Error("catalog unavailable"))
          : Promise.resolve(catalog);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve(configFile("config.toml", ""));
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const user = userEvent.setup();
    render(<ConfigPage api={{ get, post }} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("catalog unavailable");
    expect(
      screen.getByText("Configuration is unavailable. Use Retry to load it again."),
    ).toBeInTheDocument();
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    const emptyConfigs = await screen.findByText("No Named Configs found.");
    expect(emptyConfigs.closest('[data-empty-state="list"]')).toBeInTheDocument();
    expect(
      screen.queryByText("Configuration is unavailable. Use Retry to load it again."),
    ).not.toBeInTheDocument();
  });
  it("keeps a missing Managed Tenant empty without synthesizing a selector option", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Amissing&agent=codex&current=1",
    );
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Amissing&agent=codex")
        return Promise.resolve(catalog);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal")
          return Promise.resolve({
            ...configFile(body.file ?? "config.toml", ""),
            exists: false,
          });
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    render(<ConfigPage api={{ get, post }} />);
    expect(await screen.findByText("No Named Configs found.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Current Config" })).not.toBeInTheDocument();
    expect(screen.getByText("Managed Tenant not found")).toBeInTheDocument();
    expect(screen.getByText("The selected Managed Tenant does not exist.")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "config.toml content" })).not.toBeInTheDocument();
    expect(post).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Tenant: missing" })).not.toBeInTheDocument();
    const tenantFilter = screen.getByRole("button", { name: "Tenant: Not found" });
    await userEvent.setup().click(tenantFilter);
    expect(screen.queryByRole("option", { name: "missing" })).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("option", { name: "default" }));
    expect(await screen.findByRole("button", { name: "Tenant: default" })).toBeInTheDocument();
    expect(screen.queryByText("Managed Tenant not found")).not.toBeInTheDocument();
  });
  it("replaces a stale Named Config route before revealing Config files", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=missing&file=config.toml",
    );
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          current?: boolean;
          config?: string | null;
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          expect(body.current).toBe(true);
          expect(body.config).toBeNull();
          return Promise.resolve(configFile(body.file ?? "config.toml", ""));
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const onLocationChange = vi.fn((_module: string, query: URLSearchParams, replace = false) => {
      window.history[replace ? "replaceState" : "pushState"](
        null,
        "",
        `/_aibox/ui/configs?${query}`,
      );
    });
    render(<ConfigPage api={{ get, post }} onLocationChange={onLocationChange} />);
    expect(await screen.findByText("No Named Configs found.")).toBeInTheDocument();
    await waitFor(() =>
      expect(window.location.href).toContain(
        "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex",
      ),
    );
    expect(window.location.search).toBe("?tenant=managed%3Adefault&agent=codex");
    expect(screen.queryByText("Named Config missing")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(onLocationChange).toHaveBeenCalledWith("configs", expect.any(URLSearchParams), true);
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
  });
  it("retries a failed Config file reveal from the page error", async () => {
    const catalog = {
      configs: [],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    let revealAttempts = 0;
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal") {
        revealAttempts += 1;
        return revealAttempts === 1
          ? Promise.reject(new Error("file unavailable"))
          : Promise.resolve(configFile("config.toml", "retried content"));
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const user = userEvent.setup();
    render(<ConfigPage api={{ get, post }} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("file unavailable");
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "retried content",
    );
    expect(post.mock.calls.filter(([path]) => path === "/_aibox/api/configs/reveal")).toHaveLength(
      2,
    );
    expect(screen.queryByText("file unavailable")).not.toBeInTheDocument();
  });
  it("keeps Config browsing available but blocks writes during a Management Operation", async () => {
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve(configFile("config.toml", ""));
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    render(<ConfigPage api={{ get, post }} operation={activeOperation} />);
    expect(await screen.findByRole("button", { name: "Refresh Configs" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "team" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Apply Named Config team to Current Config" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete Named Config team" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create Named Config" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Changes are temporarily unavailable");
  });
  it("opens supported Named Config main files in Visual Editor and saves field projections", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=host&agent=claude&config=team&file=settings.json",
    );
    const visual = claudeVisualOptions();
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["settings.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const content = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://example.com",
        ANTHROPIC_AUTH_TOKEN: "secret",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "haiku",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "opus",
        ANTHROPIC_DEFAULT_FABLE_MODEL: "fable",
      },
      permissions: { defaultMode: "bypassPermissions" },
      skipDangerousModePermissionPrompt: true,
    });
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=host&agent=claude") return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn<(path: string, body: Record<string, unknown>) => Promise<ConfigFileData>>(
      (path) => {
        if (path === "/_aibox/api/configs/reveal")
          return Promise.resolve(configFile("settings.json", content, visual));
        if (path === "/_aibox/api/configs/save")
          return Promise.resolve(configFile("settings.json", content, visual));
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = { get, post };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    expect(await screen.findByRole("button", { name: "Visual" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    const token = screen.getByLabelText("Anthropic auth token", { selector: "input" });
    expect(token).toHaveAttribute("type", "password");
    expect(screen.queryByText("env.ANTHROPIC_AUTH_TOKEN")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Include Anthropic base URL" })).toBeNull();
    expect(screen.getByLabelText("Anthropic base URL")).toHaveAttribute("required");
    await user.click(screen.getByRole("button", { name: "Show Anthropic auth token" }));
    expect(token).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("checkbox", { name: "Include Default Haiku model" }));
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Skip dangerous mode prompt value" }),
      "__default",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/configs/save", expect.anything()),
    );
    const saveCall = post.mock.calls.find(([path]) => path === "/_aibox/api/configs/save");
    expect(saveCall?.[1].visual_options).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          path: "env.ANTHROPIC_DEFAULT_HAIKU_MODEL",
          included: false,
        }),
        expect.objectContaining({
          path: "skipDangerousModePermissionPrompt",
          included: false,
        }),
      ]),
    );
    await user.click(screen.getByRole("button", { name: "Raw" }));
    expect(screen.getByRole("textbox", { name: "settings.json content" })).toHaveValue(content);
    await user.click(screen.getByRole("button", { name: "team" }));
    expect(screen.getByRole("button", { name: "Visual" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
  });
  it("uses closed enums, Default omission, unsupported preservation, and help tooltips", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=team&file=config.toml",
    );
    const visual = [
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
        value: "future-policy",
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
        value: "workspace-write",
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
        path: "model",
        label: "Model",
        description: "Model selected for Codex sessions.",
        group: "Model & reasoning",
        value_kind: "string",
        enum_values: [],
        sensitive: false,
        required: true,
        included: true,
        value: "gpt",
      },
    ] satisfies VisualOptionFixture[];
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve(configFile("config.toml", "", visual));
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    render(<ConfigPage api={{ get, post }} />);
    const approval = await screen.findByRole("combobox", { name: "Approval policy value" });
    expect(approval).toHaveValue("future-policy");
    expect(
      within(approval).getByRole("option", { name: "Unsupported: future-policy" }),
    ).toBeTruthy();
    expect(within(approval).queryByRole("option", { name: "Custom" })).toBeNull();
    expect(within(approval).queryByRole("option", { name: "Select a value" })).toBeNull();
    expect(screen.queryByText("approval_policy")).not.toBeInTheDocument();
    const reasoning = screen.getByRole("combobox", { name: "Model reasoning effort value" });
    expect(reasoning).toHaveValue("__default");
    expect(within(reasoning).getByRole("option", { name: "Default" })).toBeTruthy();
    screen.getByRole("button", { name: "Help for Approval policy" }).focus();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Controls when Codex pauses before executing commands.",
    );
  });
  it("saves only the accepted Custom provider input fields", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=team&file=config.toml",
    );
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const customProvider = {
      included: true,
      name: "custom",
      base_url: "https://example.com/v1",
      request_proxy_route: true,
      proxy_routed: false,
    } satisfies NonNullable<ConfigFileData["custom_provider"]>;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn<(path: string, body: Record<string, unknown>) => Promise<ConfigFileData>>(
      (path) => {
        if (path === "/_aibox/api/configs/reveal")
          return Promise.resolve(configFile("config.toml", "", [], customProvider));
        if (path === "/_aibox/api/configs/save")
          return Promise.resolve(configFile("config.toml", "", [], customProvider));
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const user = userEvent.setup();
    render(<ConfigPage api={{ get, post }} />);
    const providerName = await screen.findByRole("textbox", { name: "Custom provider name" });
    await user.clear(providerName);
    await user.type(providerName, "custom-v2");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/configs/save", expect.anything()),
    );
    const saveCall = post.mock.calls.find(([path]) => path === "/_aibox/api/configs/save");
    expect(saveCall?.[1].custom_provider).toEqual({
      included: true,
      name: "custom-v2",
      base_url: "https://example.com/v1",
      proxy_routed: false,
    });
    expect(saveCall?.[1].custom_provider).not.toHaveProperty("request_proxy_route");
  });
  it("does not mark a routed Host provider dirty when it is first revealed", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=host&agent=codex&config=team&file=config.toml",
    );
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const customProvider = {
      included: true,
      name: "custom",
      base_url: "http://127.0.0.1:9923/https://example.com/v1",
      request_proxy_route: true,
      proxy_routed: false,
    } satisfies NonNullable<ConfigFileData["custom_provider"]>;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=host&agent=codex") return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve(configFile("config.toml", "", [], customProvider));
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const onDirtyChange = vi.fn();
    const api = {
      bootstrap: { version: "test", csrf_token: "token", listen: "127.0.0.1:9923" },
      get,
      post,
    };
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    await screen.findByRole("textbox", { name: "Custom provider base URL" });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));
    expect(onDirtyChange).not.toHaveBeenCalledWith(true);
  });
  it("shows non-UTF-8 Current Config as read-only downloadable bytes", async () => {
    const catalog = {
      configs: [],
      files: ["settings.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const binary = btoa(String.fromCharCode(0xff, 0x00, 0xfe));
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve({
          file: "settings.json",
          exists: true,
          revision: "binary-revision",
          content_base64: binary,
        });
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const api = { get, post };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "not valid UTF-8 and cannot be edited",
    );
    expect(screen.getByRole("button", { name: "Download raw file" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Visual" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to Configs" }));
    expect(screen.queryByRole("dialog", { name: "Unsaved changes" })).not.toBeInTheDocument();
  });
  it("restores Tenant, Agent, Named Config, and file while reporting dirty edits", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=host&agent=claude&config=team&file=settings.json",
    );
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["settings.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=host&agent=claude") {
        return Promise.resolve(catalog);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string, body: Record<string, unknown>) => {
      if (path === "/_aibox/api/configs/reveal") {
        return Promise.resolve(configFile(String(body.file), '{"model":"test"}\n'));
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const api = { get, post };
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    expect(await screen.findByRole("button", { name: "Tenant: Host Tenant" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "team" })).toHaveAttribute("aria-pressed", "true");
    const editor = await screen.findByRole("textbox", { name: "settings.json content" });
    await user.type(editor, "changed");
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    expect(post).toHaveBeenCalledWith("/_aibox/api/configs/reveal", {
      tenant: "host",
      agent: "claude",
      current: false,
      config: "team",
      file: "settings.json",
    });
  });
  it("renders row actions, protects Current, and keeps Last applied observational", async () => {
    const catalog = {
      configs: [
        { name: "custom", state: "ready" },
        {
          name: "draft",
          state: "incomplete",
          detail: "Missing required file: auth.json. Use Repair to restore this Named Config.",
        },
        { name: "broken", state: "invalid", detail: "invalid permissions" },
      ],
      files: ["config.toml", "auth.json"],
      application: {
        last_application: {
          applied: "custom",
          applied_at: "2026-08-17T00:00:00Z",
        },
        drift: "dirty",
      },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(
            configFile(body.file ?? "config.toml", "current content", [
              {
                path: "model_provider",
                label: "Model provider",
                description: "Provider used by Codex.",
                group: "Runtime",
                value_kind: "string",
                sensitive: false,
                enum_values: [],
                included: true,
                value: "openai",
              },
            ]),
          );
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const name = await screen.findByText("custom");
    const drift = screen.getByText("Dirty");
    expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    expect(drift.parentElement).toBe(name.parentElement);
    expect(name.compareDocumentPosition(drift) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      screen
        .getByRole("button", { name: "Coding Agent: Codex" })
        .querySelector('[data-icon="codex"]'),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Propagate credentials" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Current Config" })).toContainElement(
      document.querySelector('[data-icon="current-config"]'),
    );
    expect(screen.queryByText("Native Config")).not.toBeInTheDocument();
    expect(within(screen.getByRole("button", { name: "custom" })).queryByRole("img")).toBeNull();
    const warningMarker = screen.getByRole("img", {
      name: /Config warning: Incomplete Config.*Missing required file: auth.json/,
    });
    const errorMarker = screen.getByRole("img", {
      name: /Config error: Invalid Config.*invalid permissions/,
    });
    expect(screen.getByRole("button", { name: "draft" })).toHaveAccessibleDescription(
      "Config warning: Incomplete Config. Missing required file: auth.json. Use Repair to restore this Named Config.",
    );
    expect(screen.getByRole("button", { name: "broken" })).toHaveAccessibleDescription(
      "Config error: Invalid Config. invalid permissions",
    );
    await user.hover(warningMarker);
    let tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent("Warning · Incomplete Config");
    expect(tooltip).toHaveTextContent("Missing required file: auth.json");
    await user.unhover(warningMarker);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    await user.hover(errorMarker);
    tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent("Error · Invalid Config");
    expect(tooltip).toHaveTextContent("invalid permissions");
    await user.unhover(errorMarker);
    const apply = screen.getByRole("button", {
      name: "Apply Named Config custom to Current Config",
    });
    expect(apply).toBeEnabled();
    expect(apply).toHaveTextContent(/^Apply to Current Config$/);
    expect(apply.querySelector("svg")).not.toBeInTheDocument();
    const repair = screen.getByRole("button", { name: "Repair Named Config draft" });
    expect(repair).toHaveTextContent(/^Repair$/);
    expect(repair.querySelector("svg")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Apply Named Config broken to Current Config" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete Named Config broken" })).toBeInTheDocument();
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "current content",
    );
    expect(screen.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Config editing context")).toHaveTextContent(
      "TenantdefaultCoding AgentCodexConfigCurrent ConfigFileconfig.toml",
    );
    const fileContext = screen.getByTitle("config.toml + auth.json");
    expect(fileContext).toHaveTextContent("config.toml + auth.json");
    expect(fileContext).toHaveAttribute("title", "config.toml + auth.json");
    await user.click(screen.getByRole("button", { name: "Select Configs" }));
    const protectedCurrent = screen.getByRole("button", {
      name: "Current Config cannot be selected",
    });
    const selectableApplied = screen.getByRole("button", { name: "Select custom" });
    expect(protectedCurrent).toBeDisabled();
    expect(selectableApplied).toBeEnabled();
    expect(screen.getByRole("button", { name: "Create Named Config" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("3 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect custom" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Deselect draft" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Deselect broken" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(warningMarker).toBeInTheDocument();
    expect(errorMarker).toBeInTheDocument();
  });
  it("uses a no-input confirmation for one Named Config deletion", async () => {
    const catalog = {
      configs: [{ name: "custom", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(configFile(body.file ?? "config.toml", "current content"));
        }
        if (path === "/_aibox/api/configs/delete") return Promise.resolve({ deleted: ["custom"] });
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Delete Named Config custom" }));
    const dialog = screen.getByRole("dialog", { name: "Delete Named Config custom?" });
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    expect(dialog).toHaveTextContent("Current Config stays unchanged");
    await user.click(within(dialog).getByRole("button", { name: "Delete Config" }));
    expect(post).toHaveBeenCalledWith("/_aibox/api/configs/delete", {
      tenant: "managed:default",
      agent: "codex",
      configs: ["custom"],
      all: false,
      confirmation: "custom",
    });
  });
  it.each([
    ["clean", true, "Clean"],
    ["dirty", false, "Dirty"],
    ["comparison-error", false, "Comparison error"],
  ] as const)(
    "keeps Apply visible and sets its disabled state for %s drift",
    async (drift, disabled, label) => {
      const catalog = {
        configs: [{ name: "custom", state: "ready" }],
        files: ["config.toml", "auth.json"],
        application: {
          last_application: {
            applied: "custom",
            applied_at: "2026-08-17T00:00:00Z",
          },
          drift,
          detail: drift === "comparison-error" ? "could not compare Current Config" : undefined,
        },
        credential_propagation_available: false,
      } satisfies ConfigListData;
      const get = vi.fn((path: string) => {
        if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
        if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
        return Promise.reject(new Error(`Unexpected GET ${path}`));
      });
      const post = vi.fn(
        (
          _path: string,
          body: {
            file: string;
          },
        ) => Promise.resolve(configFile(body.file, "")),
      );
      const api = {
        bootstrap: { version: "test", csrf_token: "token" },
        get,
        post,
      };
      render(<ConfigPage api={api} />);
      expect(await screen.findByText(label)).toBeInTheDocument();
      const apply = screen.getByRole("button", {
        name: "Apply Named Config custom to Current Config",
      });
      if (disabled) expect(apply).toBeDisabled();
      else expect(apply).toBeEnabled();
      expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    },
  );
  it("summarizes Config Application and requires typed Host Tenant confirmation", async () => {
    const catalog = {
      configs: [{ name: "custom", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(configFile(body.file ?? "config.toml", ""));
        }
        if (path === "/_aibox/api/configs/apply") return Promise.resolve({});
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(
      await screen.findByRole("button", {
        name: "Apply Named Config custom to Current Config",
      }),
    );
    let dialog = screen.getByRole("dialog", {
      name: "Apply Named Config custom to Current Config?",
    });
    expect(dialog).toHaveTextContent("Tenant: default");
    expect(dialog).toHaveTextContent("Source: Named Config custom");
    expect(dialog).toHaveTextContent("Target: Current Config");
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Apply Named Config custom to Current Config",
      }),
    );
    dialog = screen.getByRole("dialog", {
      name: "Apply Named Config custom to Current Config?",
    });
    const confirmation = within(dialog).getByRole("textbox");
    const confirm = within(dialog).getByRole("button", { name: "Apply to Current Config" });
    expect(dialog).toHaveTextContent("Tenant: Host Tenant");
    expect(confirm).toBeDisabled();
    await user.type(confirmation, "Host Tenant");
    await user.click(confirm);
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/configs/apply", {
        tenant: "host",
        agent: "codex",
        config: "custom",
      }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "one-time projection; it is not an Active Config",
    );
  });
  it("reloads an open Current Config editor after Config Application", async () => {
    const catalog = {
      configs: [{ name: "custom", state: "ready" }],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    let applied = false;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(
            configFile(body.file ?? "config.toml", applied ? "applied content" : "old content"),
          );
        }
        if (path === "/_aibox/api/configs/apply") {
          applied = true;
          return Promise.resolve({});
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "old content",
    );
    await user.click(
      screen.getByRole("button", { name: "Apply Named Config custom to Current Config" }),
    );
    await user.click(
      within(
        screen.getByRole("dialog", { name: "Apply Named Config custom to Current Config?" }),
      ).getByRole("button", { name: "Apply to Current Config" }),
    );
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "applied content",
    );
  });
  it("shows Source missing beside an incomplete applied Config and offers Repair", async () => {
    const catalog = {
      configs: [{ name: "partial", state: "incomplete" }],
      files: ["config.toml", "auth.json"],
      application: {
        last_application: {
          applied: "partial",
          applied_at: "2026-08-17T00:00:00Z",
        },
        drift: "source-missing",
      },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        _path: string,
        body: {
          file: string;
        },
      ) => Promise.resolve(configFile(body.file, "")),
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    render(<ConfigPage api={api} />);
    const name = await screen.findByText("partial");
    const drift = screen.getByText("Source missing");
    expect(drift.parentElement).toBe(name.parentElement);
    expect(screen.getByRole("button", { name: "Repair Named Config partial" })).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Apply Named Config partial to Current Config" }),
    ).not.toBeInTheDocument();
  });
  it("shows Propagate only for Host Codex with an eligible Current credential source", async () => {
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        const query = new URL(path, "http://aibox.test").searchParams;
        const agent = query.get("agent");
        return Promise.resolve({
          configs: [],
          files: agent === "claude" ? ["settings.json"] : ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: query.get("tenant") === "host" && agent === "codex",
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        _path: string,
        body: {
          file: string;
        },
      ) => Promise.resolve(configFile(body.file, "")),
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await screen.findByRole("button", { name: "Tenant: default" });
    expect(screen.queryByRole("button", { name: "Propagate credentials" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Tenant: default" }));
    expect(
      screen.queryByRole("button", { name: "Select multiple tenants" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    const propagate = await screen.findByRole("button", { name: "Propagate credentials" });
    expect(propagate).toHaveTextContent(/^Propagate credentials$/);
    expect(propagate.querySelector("svg")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
    expect(
      screen.queryByRole("button", { name: "Select multiple Coding Agents" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Claude" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Propagate credentials" }),
      ).not.toBeInTheDocument(),
    );
  });
  it("groups Credential Propagation results and marks failed targets as partial success", async () => {
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: true,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(configFile(body.file ?? "config.toml", ""));
        }
        if (path === "/_aibox/api/configs/propagate-auth/preview") {
          return Promise.resolve({
            plan_id: "plan-1",
            preview: {
              updates: 2,
              entries: [
                { label: "default · Current", outcome: { status: "updated" } },
                { label: "work · team", outcome: { status: "updated" } },
                { label: "work · newer", outcome: { status: "newer" } },
              ],
            },
          });
        }
        if (path === "/_aibox/api/configs/propagate-auth/execute") {
          return Promise.resolve({
            counts: { updated: 1, unchanged: 1, failed: 1 },
            entries: [
              { label: "default · Current", outcome: { status: "updated" } },
              { label: "work · team", outcome: { status: "unchanged" } },
              {
                label: "work · newer",
                outcome: {
                  status: "failed",
                  reason: "target changed during propagation",
                  source_last_refresh: "2026-08-18T00:00:00Z",
                  target_last_refresh: "2026-08-19T00:00:00Z",
                },
              },
            ],
          });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Tenant: default" }));
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    await user.click(await screen.findByRole("button", { name: "Propagate credentials" }));
    let dialog = screen.getByRole("dialog", { name: "Credential Propagation preview" });
    expect(within(dialog).getByRole("heading", { name: "Updated 2" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Needs attention 1" })).toBeInTheDocument();
    await user.click(
      within(dialog).getByRole("button", { name: "Propagate 2 credential updates" }),
    );
    dialog = await screen.findByRole("dialog", { name: "Credential Propagation result" });
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Partially completed");
    expect(within(dialog).getByRole("heading", { name: "Updated 1" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Skipped 1" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Needs attention 1" })).toBeInTheDocument();
    expect(dialog).toHaveTextContent("target changed during propagation");
  });
  it("creates a DNS-label Named Config and opens its detail", async () => {
    let configs: ConfigListData["configs"] = [];
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        return Promise.resolve({
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          config?: string;
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(configFile(body.file ?? "config.toml", ""));
        }
        if (path === "/_aibox/api/configs/create") {
          configs = [{ name: body.config ?? "", state: "ready" }];
          return Promise.resolve({ created: body.config });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Create Named Config" }));
    const dialog = screen.getByRole("dialog", { name: "Create Named Config" });
    const input = within(dialog).getByRole("textbox", { name: "Named Config name" });
    await user.type(input, "Bad Name");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, "new-config");
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/configs/create", {
        tenant: "managed:default",
        agent: "codex",
        config: "new-config",
      }),
    );
    expect(await screen.findByRole("button", { name: "new-config" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("dialog", { name: "Create Named Config" })).not.toBeInTheDocument();
  });
  it("reconciles surviving selections after a non-transactional batch deletion failure", async () => {
    let configs: ConfigListData["configs"] = [
      { name: "first", state: "ready" },
      { name: "second", state: "ready" },
    ];
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        return Promise.resolve({
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          return Promise.resolve(configFile(body.file ?? "config.toml", ""));
        }
        if (path === "/_aibox/api/configs/delete") {
          configs = [{ name: "second", state: "ready" }];
          return Promise.reject(new Error("second Named Config could not be deleted"));
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await screen.findByRole("button", { name: "first" });
    await user.click(screen.getByRole("button", { name: "Select Configs" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Named Configs" }));
    const dialog = screen.getByRole("dialog", { name: "Delete selected Named Configs?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete selected" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "second Named Config could not be deleted",
    );
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect second" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("button", { name: "first" })).not.toBeInTheDocument();
  });
  it("keeps Codex files visible together and saves every dirty file before switching", async () => {
    const catalog = {
      configs: [{ name: "other", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file: string;
          current?: boolean;
          config?: string;
          content_base64?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          const owner = body.current ? "current" : body.config;
          return Promise.resolve(configFile(body.file, `${owner}:${body.file}`));
        }
        if (path === "/_aibox/api/configs/save") {
          return Promise.resolve({
            ...configFile(body.file, "saved"),
            content_base64: body.content_base64,
          });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    expect(editor).toHaveValue("current:config.toml");
    await user.clear(editor);
    await user.type(editor, "changed main");
    const authEditor = await screen.findByRole("textbox", { name: "auth.json content" });
    expect(authEditor).toHaveValue("current:auth.json");
    await user.clear(authEditor);
    await user.type(authEditor, "changed auth");
    await user.click(screen.getByRole("button", { name: "other" }));
    const dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Save and continue" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        "/_aibox/api/configs/save",
        expect.objectContaining({ current: true, file: "config.toml" }),
      ),
    );
    expect(post).toHaveBeenCalledWith(
      "/_aibox/api/configs/save",
      expect.objectContaining({ current: true, file: "auth.json" }),
    );
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "other:config.toml",
    );
    expect(await screen.findByRole("textbox", { name: "auth.json content" })).toHaveValue(
      "other:auth.json",
    );
  });
  it("does not carry saved feedback into the Config selected after Save and continue", async () => {
    const catalog = {
      configs: [{ name: "other", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: {
          file: string;
          current?: boolean;
          config?: string;
          content_base64?: string;
        },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          const owner = body.current ? "current" : body.config;
          return Promise.resolve({
            ...configFile(body.file, `${owner}:${body.file}`),
            exists: !body.current,
          });
        }
        if (path === "/_aibox/api/configs/save") {
          return Promise.resolve({
            ...configFile(body.file, "saved"),
            content_base64: body.content_base64,
          });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "other" }));
    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    expect(editor).toHaveValue("other:config.toml");
    await user.clear(editor);
    await user.type(editor, "changed named Config");
    await user.click(screen.getByRole("button", { name: "Current Config" }));
    const dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Save and continue" }));
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "current:config.toml",
    );
    expect(screen.getByRole("region", { name: "config.toml editor" })).toHaveTextContent(
      "New file",
    );
    expect(screen.queryByRole("button", { name: "Saved" })).not.toBeInTheDocument();
  });
});
