import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigFileData, ConfigListData } from "@/api/configs";
import {
  claudeVisualOptions,
  configFile,
  type VisualOptionFixture,
} from "@/features/configs/testFixtures";
import { ConfigPage, configApi, revealConfigFiles } from "@/features/configs/testHarness";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});

function visualCodexNamedConfig() {
  const customProvider = {
    included: true,
    name: "custom",
    base_url: "https://example.com/v1",
    request_proxy_route: true,
    proxy_routed: false,
  } satisfies NonNullable<ConfigFileData["custom_provider"]>;
  const files: Record<string, ConfigFileData> = {
    "config.toml": configFile("config.toml", "", [], customProvider),
    "auth.json": {
      ...configFile("auth.json", '{"OPENAI_API_KEY":"sk-old"}'),
      auth: { mode: "api-key", api_key: "sk-old", extra_fields: false, warnings: [] },
    },
  };
  return {
    catalog: {
      configs: [{ name: "freebie", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData,
    revealConfigFile: (file: string) => Promise.resolve(files[file] ?? files["config.toml"]),
  };
}

async function editVisualCodexNamedFiles(user: ReturnType<typeof userEvent.setup>) {
  const providerName = await screen.findByRole("textbox", { name: "Custom provider name" });
  await user.clear(providerName);
  await user.type(providerName, "custom-v2");
  const apiKey = await screen.findByLabelText("OpenAI API key");
  await user.clear(apiKey);
  await user.type(apiKey, "sk-new");
  expect(await screen.findByRole("button", { name: "Save all" })).toBeEnabled();
}

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
    await revealConfigFiles(user);
    expect(await screen.findByRole("button", { name: "Visual" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("heading", { name: "Named Config team" })).toBeInTheDocument();
    // The notice states both standing conditions, and Visual mode does not hide it.
    expect(
      screen.getByText(
        "Edits write to the real Host Home · Native content may contain credentials and is shown without redaction.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Host risk/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Raw" }));
    expect(screen.getByText(/Edits write to the real Host Home/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Visual" }));
    expect(
      Array.from(document.querySelectorAll("article[role='group']")).map(
        (group) => group.querySelector("span")?.textContent,
      ),
    ).toEqual([
      "Base URL",
      "Auth token",
      "Default permission mode",
      "Skip dangerous mode prompt",
      "Default Haiku model",
      "Default Sonnet model",
      "Default Opus model",
      "Default Fable model",
    ]);
    const token = screen.getByLabelText("Auth token", { selector: "input" });
    expect(token).toHaveAttribute("type", "password");
    // The label carries its native path so Visual, Raw, and differences agree.
    expect(screen.getByText("env.ANTHROPIC_AUTH_TOKEN").tagName).toBe("CODE");
    expect(screen.queryByRole("checkbox", { name: "Optional Base URL" })).toBeNull();
    expect(screen.getByLabelText("Base URL")).toHaveAttribute("required");
    expect(screen.getAllByText("Required").length).toBeGreaterThan(0);
    const permissionMode = screen.getByRole("combobox", { name: "Default permission mode value" });
    expect(permissionMode).toHaveTextContent("bypassPermissions");
    await user.click(permissionMode);
    const permissionList = screen.getByRole("listbox", {
      name: "Default permission mode single selection",
    });
    expect(
      within(permissionList)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["default", "acceptEdits", "plan", "auto", "dontAsk", "bypassPermissions"]);
    expect(within(permissionList).queryByRole("option", { name: "manual" })).toBeNull();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Show Auth token" }));
    expect(token).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("checkbox", { name: "Optional Default Haiku model" }));
    await user.click(screen.getByRole("checkbox", { name: "Optional Skip dangerous mode prompt" }));
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
  it.each(["bypassPermissions", "default"])(
    "omits unavailable Skip fields starting from %s",
    async (initialMode) => {
      window.history.replaceState(
        null,
        "",
        "/_aibox/ui/configs?tenant=managed%3Adefault&agent=claude&config=team&file=settings.json",
      );
      const visual = claudeVisualOptions().map((field) =>
        field.path === "permissions.defaultMode" ? { ...field, value: initialMode } : field,
      );
      const { api, saveConfigFile } = configApi({
        listConfigs: () =>
          Promise.resolve({
            configs: [{ name: "team", state: "ready" }],
            files: ["settings.json"],
            application: { last_application: null, drift: "untracked" },
            credential_propagation_available: false,
          }),
        revealConfigFile: () => Promise.resolve(configFile("settings.json", "{}", visual)),
        saveConfigFile: () => Promise.resolve(configFile("settings.json", "{}", visual)),
      });
      const user = userEvent.setup();
      render(<ConfigPage api={api} />);
      await revealConfigFiles(user);
      const permission = await screen.findByRole("combobox", {
        name: "Default permission mode value",
      });
      const skipName = "Optional Skip dangerous mode prompt";
      if (initialMode === "bypassPermissions") {
        expect(screen.getByRole("checkbox", { name: skipName })).toBeChecked();
        await user.click(permission);
        await user.click(screen.getByRole("option", { name: "default" }));
      }
      expect(screen.queryByRole("checkbox", { name: skipName })).toBeNull();
      expect(saveConfigFile).not.toHaveBeenCalled();
      await user.click(permission);
      await user.click(screen.getByRole("option", { name: "bypassPermissions" }));
      expect(screen.getByRole("checkbox", { name: skipName })).not.toBeChecked();
      expect(
        screen.getByRole("combobox", { name: "Skip dangerous mode prompt value" }),
      ).toBeDisabled();
      await user.click(screen.getByRole("checkbox", { name: skipName }));
      expect(
        screen.getByRole("combobox", { name: "Skip dangerous mode prompt value" }),
      ).toBeEnabled();
      await user.click(permission);
      await user.click(screen.getByRole("option", { name: "default" }));
      await user.type(screen.getByLabelText("Base URL", { selector: "input" }), "/v2");
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(saveConfigFile).toHaveBeenCalled());
      expect(saveConfigFile.mock.calls[0]?.[1].visualOptions).toHaveLength(8);
      expect(saveConfigFile.mock.calls[0]?.[1].visualOptions).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ path: "skipDangerousModePermissionPrompt", included: false }),
        ]),
      );
    },
  );
  it("uses closed enums, Optional omission, unsupported preservation, without help tooltips", async () => {
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
        enum_values: ["low", "medium", "high", "xhigh", "max", "ultra"],
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
    const user = userEvent.setup();
    await revealConfigFiles(user);
    const approval = await screen.findByRole("combobox", { name: "Approval policy value" });
    expect(screen.getByRole("heading", { name: "Named Config team" })).toBeInTheDocument();
    expect(
      screen.getByText("Native content may contain credentials and is shown without redaction."),
    ).toBeInTheDocument();
    expect(approval).toHaveTextContent("Unsupported: future-policy");
    await user.click(approval);
    const approvalList = screen.getByRole("listbox", { name: "Approval policy single selection" });
    expect(
      within(approvalList).getByRole("option", { name: "Unsupported: future-policy" }),
    ).toBeTruthy();
    expect(within(approvalList).queryByRole("option", { name: "Custom" })).toBeNull();
    expect(within(approvalList).queryByRole("option", { name: "Select a value" })).toBeNull();
    expect(screen.getByText("approval_policy").tagName).toBe("CODE");
    await user.keyboard("{Escape}");
    const reasoning = screen.getByRole("combobox", { name: "Model reasoning effort value" });
    const includeReasoning = screen.getByRole("checkbox", {
      name: "Optional Model reasoning effort",
    });
    expect(includeReasoning).not.toBeChecked();
    expect(reasoning).toBeDisabled();
    expect(reasoning).not.toHaveTextContent("Default");
    await user.click(includeReasoning);
    expect(includeReasoning).toBeChecked();
    expect(reasoning).toBeEnabled();
    expect(reasoning).toHaveTextContent("low");
    await user.click(reasoning);
    const reasoningList = screen.getByRole("listbox", {
      name: "Model reasoning effort single selection",
    });
    expect(within(reasoningList).queryByRole("option", { name: "Default" })).toBeNull();
    expect(within(reasoningList).getByRole("option", { name: "max" })).toBeTruthy();
    expect(within(reasoningList).getByRole("option", { name: "ultra" })).toBeTruthy();
    expect(within(reasoningList).queryByRole("option", { name: "minimal" })).toBeNull();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: /^Help for/ })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Execution & permissions" })).toBeNull();
    await user.click(screen.getByText("Sandbox mode", { exact: true }));
    expect(screen.getByRole("combobox", { name: "Sandbox mode value" })).not.toHaveFocus();
    expect(screen.queryByRole("listbox")).toBeNull();
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
    await revealConfigFiles(user);
    const providerName = await screen.findByRole("textbox", { name: "Custom provider name" });
    expect(screen.getByRole("checkbox", { name: "Optional Custom provider" })).toBeChecked();
    expect(screen.queryByText("Name", { exact: true })).toBeNull();
    expect(screen.getAllByText("Required")).toHaveLength(1);
    await user.clear(providerName);
    await user.type(providerName, "custom-v2");
    await user.click(screen.getByText("Custom provider", { exact: true }));
    expect(screen.getByRole("checkbox", { name: "Optional Custom provider" })).toBeChecked();
    expect(screen.getByRole("textbox", { name: "Custom provider name" })).toHaveValue("custom-v2");
    await user.click(screen.getByRole("checkbox", { name: "Optional Custom provider" }));
    expect(screen.getByRole("checkbox", { name: "Optional Custom provider" })).not.toBeChecked();
    expect(screen.queryByRole("textbox", { name: "Custom provider name" })).toBeNull();
    expect(screen.queryByRole("textbox", { name: "Custom provider base URL" })).toBeNull();
    await user.click(screen.getByRole("checkbox", { name: "Optional Custom provider" }));
    expect(screen.getByRole("textbox", { name: "Custom provider name" })).toHaveValue("custom-v2");
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
  it("refuses an invalid Visual save in the file, marks the field, and clears on edit", async () => {
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
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
    const baseUrl = await screen.findByRole("textbox", { name: "Custom provider base URL" });
    expect(screen.getByText("model_providers.custom.base_url").tagName).toBe("CODE");
    expect(screen.queryByText(/Routed through the Request Proxy/)).toBeNull();
    await user.clear(baseUrl);
    await user.type(baseUrl, "not a url");
    await user.click(screen.getByRole("button", { name: "Save" }));
    const region = screen.getByRole("region", { name: "config.toml editor" });
    expect(within(region).getByRole("alert", { name: "" }).textContent).toBe(
      "Base URL must be a valid HTTP or HTTPS URL.",
    );
    expect(baseUrl).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("textbox", { name: "Custom provider name" })).not.toHaveAttribute(
      "aria-invalid",
    );
    expect(saveConfigFile).not.toHaveBeenCalled();
    await user.type(baseUrl, "x");
    expect(within(region).queryByRole("alert")).toBeNull();
    expect(baseUrl).not.toHaveAttribute("aria-invalid");
    await user.clear(baseUrl);
    await user.type(baseUrl, "https://api.example.com/v1");
    await user.click(
      screen.getByRole("checkbox", { name: "Route Custom provider through Request Proxy" }),
    );
    expect(screen.getByText(/Routed through the Request Proxy at/)).toHaveTextContent(
      "http://host.docker.internal:3000/",
    );
  });
  it("saves every dirty Visual Codex file in one Save all click", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=freebie&file=config.toml",
    );
    const { catalog, revealConfigFile } = visualCodexNamedConfig();
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => revealConfigFile(target.file),
      saveConfigFile: async (target, input) => ({
        ...(await revealConfigFile(target.file)),
        content_base64: input.contentBase64,
      }),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
    await editVisualCodexNamedFiles(user);
    await user.click(screen.getByRole("button", { name: "Save all" }));
    await waitFor(() => expect(saveConfigFile).toHaveBeenCalledTimes(2));
    expect(saveConfigFile.mock.calls.map(([target]) => target.file)).toEqual([
      "auth.json",
      "config.toml",
    ]);
  });
  it("names the dirty file in its own header and holds Save all until two files are dirty", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=freebie&file=config.toml",
    );
    const { catalog, revealConfigFile } = visualCodexNamedConfig();
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => revealConfigFile(target.file),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
    const main = await screen.findByRole("region", { name: "config.toml editor" });
    const auth = await screen.findByRole("region", { name: "auth.json editor" });
    // At rest an existing file's header carries no caption and no marker.
    expect(within(main).queryByText("Existing file")).not.toBeInTheDocument();
    expect(document.querySelector('[class*="configFileSectionFocused"]')).toBeNull();
    // One dirty file: its own header says so, and no Save all appears.
    const apiKey = await screen.findByLabelText("OpenAI API key");
    await user.clear(apiKey);
    await user.type(apiKey, "sk-new");
    expect(screen.getByText("1 unsaved file")).toBeInTheDocument();
    expect(within(auth).getByText("Unsaved changes")).toBeInTheDocument();
    expect(within(main).queryByText("Unsaved changes")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save all" })).not.toBeInTheDocument();
    // Two dirty files: Save all has a job of its own.
    const providerName = await screen.findByRole("textbox", { name: "Custom provider name" });
    await user.clear(providerName);
    await user.type(providerName, "custom-v2");
    expect(screen.getByText("2 unsaved files")).toBeInTheDocument();
    expect(within(main).getByText("Unsaved changes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save all" })).toBeEnabled();
  });
  it("keeps the Custom provider Save gate on the main file while auth.json is dirty", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&config=freebie&file=config.toml",
    );
    const { catalog, revealConfigFile } = visualCodexNamedConfig();
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => revealConfigFile(target.file),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
    await editVisualCodexNamedFiles(user);
    await user.click(
      within(screen.getByRole("region", { name: "config.toml editor" })).getByRole("button", {
        name: "Save",
      }),
    );
    expect(
      await screen.findByText("Save auth.json before saving a Custom provider configuration."),
    ).toBeInTheDocument();
    expect(saveConfigFile).not.toHaveBeenCalled();
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
    const user = userEvent.setup();
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    await revealConfigFiles(user);
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
    await revealConfigFiles(user);
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, '{"model":"test"}\n')),
    });
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    await revealConfigFiles(user);
    expect(await screen.findByRole("button", { name: "Tenant: Host" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "team" })).toHaveAttribute("aria-pressed", "true");
    const editor = await screen.findByRole("textbox", { name: "settings.json content" });
    await user.type(editor, "changed");
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
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
    await revealConfigFiles(user);
    const name = await screen.findByText("custom");
    const drift = screen.getByText("Differs");
    const current = screen.getByRole("button", { name: "Current Config" });
    expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    expect(screen.queryByText(/not an Active Config/)).not.toBeInTheDocument();
    expect(screen.queryByText("Dirty")).not.toBeInTheDocument();
    expect(current).toHaveAccessibleDescription("Last applied custom · differs");
    expect(within(current).getByText("Last applied custom · differs")).toBeInTheDocument();
    expect(within(current).queryByText("Differs")).not.toBeInTheDocument();
    expect(drift.parentElement).toBe(name.parentElement);
    expect(name.compareDocumentPosition(drift) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const agentIcon = screen
      .getByRole("button", { name: "Coding Agent: Codex" })
      .querySelector<HTMLElement>('[data-icon="openai"]');
    expect(agentIcon).toBeInTheDocument();
    expect(agentIcon?.style.getPropertyValue("--brand-icon-size")).toBe("14px");
    expect(screen.queryByRole("button", { name: "Propagate credentials" })).not.toBeInTheDocument();
    expect(current).toContainElement(document.querySelector('[data-icon="current-config"]'));
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
    expect(screen.getByRole("heading", { name: "Current Config" })).toBeInTheDocument();
    expect(
      screen.getByText("Native content may contain credentials and is shown without redaction."),
    ).toBeInTheDocument();
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
