import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigFileData, ConfigListData } from "@/api/configs";
import {
  claudeVisualOptions,
  configFile,
  type VisualOptionFixture,
} from "@/features/configs/testFixtures";
import { ConfigPage, configApi } from "@/features/configs/testHarness";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("ConfigPage", () => {
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
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("settings.json", content, visual)),
      saveConfigFile: () => Promise.resolve(configFile("settings.json", content, visual)),
    });
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
    await waitFor(() => expect(saveConfigFile).toHaveBeenCalled());
    const saveInput = saveConfigFile.mock.calls[0]?.[1];
    expect(saveInput?.visualOptions).toEqual(
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("config.toml", "", visual)),
    });
    render(<ConfigPage api={api} />);
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
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("config.toml", "", [], customProvider)),
      saveConfigFile: () => Promise.resolve(configFile("config.toml", "", [], customProvider)),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const providerName = await screen.findByRole("textbox", { name: "Custom provider name" });
    await user.clear(providerName);
    await user.type(providerName, "custom-v2");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(saveConfigFile).toHaveBeenCalled());
    const saveInput = saveConfigFile.mock.calls[0]?.[1];
    expect(saveInput?.customProvider).toEqual({
      included: true,
      name: "custom-v2",
      base_url: "https://example.com/v1",
      proxy_routed: false,
    });
    expect(saveInput?.customProvider).not.toHaveProperty("request_proxy_route");
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
    const onDirtyChange = vi.fn();
    const { api } = configApi({
      bootstrap: { listen: "127.0.0.1:9923" },
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("config.toml", "", [], customProvider)),
    });
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
    const binary = btoa(String.fromCharCode(0xff, 0x00, 0xfe));
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () =>
        Promise.resolve({
          file: "settings.json",
          exists: true,
          revision: "binary-revision",
          content_base64: binary,
        }),
    });
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
    const { api, revealConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, '{"model":"test"}\n')),
    });
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    expect(await screen.findByRole("button", { name: "Tenant: Host Tenant" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "team" })).toHaveAttribute("aria-pressed", "true");
    const editor = await screen.findByRole("textbox", { name: "settings.json content" });
    await user.type(editor, "changed");
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    expect(revealConfigFile).toHaveBeenCalledWith({
      tenant: { kind: "host" },
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) =>
        Promise.resolve(
          configFile(target.file, "current content", [
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
        ),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const name = await screen.findByText("custom");
    const drift = screen.getByText("Dirty");
    expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    expect(drift.parentElement).toBe(name.parentElement);
    expect(name.compareDocumentPosition(drift) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const agentIcon = screen
      .getByRole("button", { name: "Coding Agent: Codex" })
      .querySelector<HTMLElement>('[data-icon="openai"]');
    expect(agentIcon).toBeInTheDocument();
    expect(agentIcon?.style.getPropertyValue("--brand-icon-size")).toBe("14px");
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
    expect(apply).toHaveTextContent(/^Apply$/);
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
});
