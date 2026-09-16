import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigListData } from "@/api/configs";
import { configFile } from "@/features/configs/testFixtures";
import { deferred } from "@/test/deferred";
import { ConfigPage, configApi, revealConfigFiles } from "@/features/configs/testHarness";
import layout from "@/shared/ui/layout/catalog.module.css";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState(null, "", "/");
});
describe("ConfigPage", () => {
  it("keeps Codex files visible together and saves every dirty file before switching", async () => {
    const catalog = {
      configs: [{ name: "other", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => {
        const owner = target.current ? "current" : target.config;
        return Promise.resolve(configFile(target.file, `${owner}:${target.file}`));
      },
      saveConfigFile: (target, input) =>
        Promise.resolve({
          ...configFile(target.file, "saved"),
          content_base64: input.contentBase64,
        }),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
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
      expect(saveConfigFile).toHaveBeenCalledWith(
        expect.objectContaining({ current: true, file: "config.toml" }),
        expect.any(Object),
      ),
    );
    expect(saveConfigFile).toHaveBeenCalledWith(
      expect.objectContaining({ current: true, file: "auth.json" }),
      expect.any(Object),
    );
    await revealConfigFiles(user);
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => {
        const owner = target.current ? "current" : target.config;
        return Promise.resolve({
          ...configFile(target.file, `${owner}:${target.file}`),
          exists: !target.current,
        });
      },
      saveConfigFile: (target, input) =>
        Promise.resolve({
          ...configFile(target.file, "saved"),
          content_base64: input.contentBase64,
        }),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "other" }));
    await revealConfigFiles(user);
    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    expect(editor).toHaveValue("other:config.toml");
    await user.clear(editor);
    await user.type(editor, "changed named Config");
    await user.click(screen.getByRole("button", { name: "Current Config" }));
    const dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Save and continue" }));
    await revealConfigFiles(user);
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "current:config.toml",
    );
    expect(screen.getByRole("region", { name: "config.toml editor" })).toHaveTextContent(
      "New file",
    );
    expect(screen.queryByRole("button", { name: "Saved" })).not.toBeInTheDocument();
  });

  /*
   * The scope pickers were disabled while the catalog reloaded, so the focus
   * SelectionMenu handed back to its trigger fell to <body> a frame later.
   * A superseded reload is already cancelled by the catalog hook, so the
   * pickers stay live and keep focus like the same pickers on Sessions.
   */
  it("keeps the Tenant picker focused and live while its catalog loads", async () => {
    const empty = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const slow = deferred<ConfigListData>();
    let calls = 0;
    const { api } = configApi({
      listConfigs: () => (calls++ === 0 ? Promise.resolve(empty) : slow.promise),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Tenant: default" }));
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    const trigger = screen.getByRole("button", { name: "Tenant: Host" });
    expect(trigger).toHaveFocus();
    expect(trigger).toBeEnabled();
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeEnabled();
    slow.resolve(empty);
    await waitFor(() => expect(screen.getByRole("button", { name: "Tenant: Host" })).toHaveFocus());
  });

  it("opens the Named Configs catalog without inspecting Current Config", async () => {
    window.history.replaceState(null, "", "/_aibox/ui/configs?tenant=host&agent=codex&named=1");
    const catalog = {
      configs: [
        { name: "ag-github", state: "ready" },
        { name: "freebie", state: "ready" },
      ],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => {
        const owner = target.current ? "current" : target.config;
        return Promise.resolve(configFile(target.file, `${owner}:${target.file}`));
      },
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    expect(await screen.findByRole("button", { name: "ag-github" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Current Config" })).not.toHaveAttribute(
      "aria-pressed",
    );
    expect(screen.getByRole("heading", { name: "Named Configs" })).toBeInTheDocument();
    expect(
      screen.getByText("Select Current Config or a Named Config to inspect its files."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "config.toml content" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "ag-github" }));
    await revealConfigFiles(user);
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "ag-github:config.toml",
    );
    expect(window.location.search).toContain("config=ag-github");
    expect(window.location.search).not.toContain("named=1");
  });

  it("does not mark Current Config as inspected on a one-pane catalog until it is opened", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    );
    const catalog = {
      configs: [{ name: "freebie", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) =>
        Promise.resolve(configFile(target.file, `${target.current ? "current" : "named"}:file`)),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const current = await screen.findByRole("button", { name: "Current Config" });
    expect(current).not.toHaveAttribute("aria-pressed");
    expect(current.closest("div")).not.toHaveClass(layout.rowInspected);

    await user.click(current);
    expect(current).toHaveAttribute("aria-pressed", "true");
    expect(current.closest("div")).toHaveClass(layout.rowInspected);
  });

  it("keeps a Named Configs catalog route when the catalog is empty", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&named=1",
    );
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
    });
    render(<ConfigPage api={api} />);
    expect(await screen.findByText("No Named Configs found.")).toBeInTheDocument();
    expect(window.location.search).toBe("?tenant=managed%3Adefault&agent=codex&named=1");
    expect(screen.getByRole("button", { name: "Current Config" })).not.toHaveAttribute(
      "aria-pressed",
    );
    expect(screen.queryByRole("textbox", { name: "config.toml content" })).not.toBeInTheDocument();
  });

  it("uses Unsaved changes when the shell asks to leave a dirty editor", async () => {
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api, saveConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "current content")),
      saveConfigFile: (target, input) =>
        Promise.resolve({
          ...configFile(target.file, "saved"),
          content_base64: input.contentBase64,
        }),
    });
    const onCancelLeave = vi.fn();
    const onContinueLeave = vi.fn();
    const user = userEvent.setup();
    const view = render(<ConfigPage api={api} />);
    await revealConfigFiles(user);
    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    await user.type(editor, "changed");
    view.rerender(
      <ConfigPage
        api={api}
        pendingLeave
        onCancelLeave={onCancelLeave}
        onContinueLeave={onContinueLeave}
      />,
    );
    const dialog = await screen.findByRole("dialog", { name: "Unsaved changes" });
    expect(dialog).toHaveTextContent("Save changes to config.toml before continuing?");
    await user.click(within(dialog).getByRole("button", { name: "Save and continue" }));
    await waitFor(() => expect(saveConfigFile).toHaveBeenCalled());
    expect(onContinueLeave).toHaveBeenCalled();
    expect(onCancelLeave).not.toHaveBeenCalled();
  });
});
