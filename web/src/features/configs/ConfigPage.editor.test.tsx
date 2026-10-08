import { ConfigPage as ConfigPageView } from "@/features/configs/ConfigPage";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigFileData, ConfigListData } from "@/api/configs";
import { claudeVisualOptions, configFile } from "@/features/configs/testFixtures";
import { deferred } from "@/test/deferred";
import { ConfigPage, configApi } from "@/features/configs/testHarness";

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
    const snapshot = configFile("settings.json", content, visual);
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(snapshot),
      saveConfigFile: () => Promise.resolve(snapshot),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
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
    await user.click(screen.getByRole("checkbox", { name: "Optional Default Haiku model" }));
    await user.click(screen.getByRole("checkbox", { name: "Optional Skip dangerous mode prompt" }));
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(saveConfigFile).toHaveBeenCalled());
    const omitted = new Set([
      "env.ANTHROPIC_DEFAULT_HAIKU_MODEL",
      "skipDangerousModePermissionPrompt",
    ]);
    expect(saveConfigFile).toHaveBeenCalledExactlyOnceWith(
      {
        tenant: { kind: "host" },
        agent: "claude",
        current: false,
        config: "team",
        file: "settings.json",
      },
      {
        revision: snapshot.revision,
        contentBase64: snapshot.content_base64,
        visualOptions: visual.map(({ path, value }) => ({
          path,
          included: !omitted.has(path),
          value,
        })),
      },
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
  it("names a dirty mode switch as a switch, not a leave, and keeps the draft on Cancel", async () => {
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
    const name = await screen.findByRole("textbox", { name: "Custom provider name" });
    await user.type(name, "-edited");
    await user.click(screen.getByRole("button", { name: "Raw" }));
    const dialog = screen.getByRole("dialog", { name: "Switch to Raw?" });
    expect(dialog).toHaveTextContent(
      "Your unsaved edits to config.toml only exist in the Visual editor. Save them first, or discard them to switch.",
    );
    expect(screen.queryByRole("dialog", { name: "Unsaved changes" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "Visual" })).toHaveAttribute("aria-pressed", "true");
    expect(name).toHaveValue("custom-edited");
    await user.click(screen.getByRole("button", { name: "Raw" }));
    await user.click(
      within(screen.getByRole("dialog", { name: "Switch to Raw?" })).getByRole("button", {
        name: "Discard and switch",
      }),
    );
    expect(screen.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
    expect(saveConfigFile).not.toHaveBeenCalled();
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, '{"model":"test"}\n')),
    });
    const onDirtyChange = vi.fn();
    const user = userEvent.setup();
    render(<ConfigPage api={api} onDirtyChange={onDirtyChange} />);
    expect(await screen.findByRole("button", { name: "Tenant: Host" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent: Claude" })).toBeInTheDocument();
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
      .getByRole("button", { name: "Agent: Codex" })
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

it("preserves the editing scope when only the routed file changes", async () => {
  const { api, revealConfigFile, listConfigs } = configApi({
    revealConfigFile: ({ file }) => Promise.resolve(configFile(file, "original")),
  });
  const onLocationChange = vi.fn();
  const search = "?tenant=host&agent=codex&current=1&file=";
  const view = render(
    <ConfigPageView
      api={api}
      search={search + "config.toml"}
      onLocationChange={onLocationChange}
    />,
  );
  const user = userEvent.setup();
  const input = await screen.findByRole("textbox", { name: "config.toml content" });
  await user.clear(input);
  await user.type(input, "unsaved draft");
  view.rerender(
    <ConfigPageView api={api} search={search + "auth.json"} onLocationChange={onLocationChange} />,
  );
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "unsaved draft",
    ),
  );
  expect(revealConfigFile).toHaveBeenCalledTimes(2);
  expect(listConfigs).toHaveBeenCalledTimes(1);
});

it("corrects stale routes using the current catalog without refetching it", async () => {
  const { api, listConfigs } = configApi({
    revealConfigFile: ({ file }) => Promise.resolve(configFile(file, "original")),
  });
  const onLocationChange = vi.fn();
  const search = "?tenant=host&agent=codex&current=1&file=";
  const view = render(
    <ConfigPageView
      api={api}
      search={search + "config.toml"}
      onLocationChange={onLocationChange}
    />,
  );
  const user = userEvent.setup();
  const input = await screen.findByRole("textbox", { name: "config.toml content" });
  await user.clear(input);
  await user.type(input, "unsaved draft");
  view.rerender(
    <ConfigPageView
      api={api}
      search={search + "missing.json"}
      onLocationChange={onLocationChange}
    />,
  );
  await waitFor(() =>
    expect(onLocationChange).toHaveBeenLastCalledWith(
      new URLSearchParams("tenant=host&agent=codex&current=1&file=config.toml"),
      true,
    ),
  );
  expect(screen.getByRole("textbox", { name: "config.toml content" })).toHaveValue("unsaved draft");
  view.rerender(
    <ConfigPageView
      api={api}
      search="?tenant=host&agent=codex&config=deleted"
      onLocationChange={onLocationChange}
    />,
  );
  await waitFor(() =>
    expect(onLocationChange).toHaveBeenLastCalledWith(
      new URLSearchParams("tenant=host&agent=codex"),
      true,
    ),
  );
  expect(listConfigs).toHaveBeenCalledTimes(1);
});

it("does not correct a new Agent route using the previous Agent catalog", async () => {
  const nextCatalog = deferred<ConfigListData>();
  const catalog: ConfigListData = {
    configs: [],
    files: ["config.toml", "auth.json"],
    application: { last_application: null, drift: "untracked" },
    credential_propagation_available: false,
  };
  const { api, listConfigs } = configApi({
    listConfigs: (_tenant, agent) =>
      agent === "codex" ? Promise.resolve(catalog) : nextCatalog.promise,
    revealConfigFile: ({ file }) => Promise.resolve(configFile(file, "original")),
  });
  const onLocationChange = vi.fn();
  const view = render(
    <ConfigPageView
      api={api}
      search="?tenant=host&agent=codex&current=1"
      onLocationChange={onLocationChange}
    />,
  );
  await screen.findByRole("textbox", { name: "config.toml content" });
  view.rerender(
    <ConfigPageView
      api={api}
      search="?tenant=host&agent=claude&config=team&file=settings.json"
      onLocationChange={onLocationChange}
    />,
  );
  await waitFor(() => expect(listConfigs).toHaveBeenCalledTimes(2));
  expect(onLocationChange).not.toHaveBeenCalled();
  await act(() => {
    nextCatalog.resolve({
      ...catalog,
      configs: [{ name: "team", state: "ready" }],
      files: ["settings.json"],
    });
    return nextCatalog.promise;
  });
  await screen.findByRole("textbox", { name: "settings.json content" });
  expect(onLocationChange).not.toHaveBeenCalled();
});
