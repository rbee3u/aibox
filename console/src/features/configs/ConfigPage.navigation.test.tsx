import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { ConfigListData } from "@/api/configs";
import { configFile } from "@/features/configs/testFixtures";
import { ConfigPage, configApi } from "@/features/configs/testHarness";

afterEach(() => {
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
    const { api, revealConfigFile } = configApi({
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
    expect(revealConfigFile).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "ag-github" }));
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "ag-github:config.toml",
    );
    expect(window.location.search).toContain("config=ag-github");
    expect(window.location.search).not.toContain("named=1");
    expect(revealConfigFile).toHaveBeenCalled();
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
    const { api, revealConfigFile } = configApi({
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
    expect(revealConfigFile).not.toHaveBeenCalled();
  });
});
