import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { ConfigListData } from "@/api/configs";
import { configFile } from "@/features/configs/testFixtures";
import { ConfigPage, configApi } from "@/features/configs/testHarness";
import actionStyles from "@/shared/ui/ActionButton.module.css";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("ConfigPage", () => {
  it("uses a no-input confirmation for one Named Config deletion", async () => {
    const catalog = {
      configs: [{ name: "custom", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api, deleteConfigs } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "current content")),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Delete Named Config custom" }));
    const dialog = screen.getByRole("dialog", { name: "Delete Named Config custom?" });
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    expect(dialog).toHaveTextContent("Current Config is unchanged");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(deleteConfigs).toHaveBeenCalledWith({ kind: "managed", name: "default" }, "codex", [
      "custom",
    ]);
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
      const { api } = configApi({
        listConfigs: () => Promise.resolve(catalog),
        revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
      });
      render(<ConfigPage api={api} />);
      const statusElement = (await screen.findByText(label)).closest("[data-status-variant]");
      expect(statusElement).toHaveAttribute(
        "data-status-variant",
        drift === "clean" ? "inline" : "badge",
      );
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
    const { api, applyConfig } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
      applyConfig: () => Promise.resolve(),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(
      await screen.findByRole("button", {
        name: "Apply Named Config custom to Current Config",
      }),
    );
    let dialog = screen.getByRole("dialog", {
      name: "Apply custom to Current Config?",
    });
    expect(within(dialog).getByText("default")).toBeInTheDocument();
    expect(within(dialog).getByText("Named Config custom")).toBeInTheDocument();
    expect(within(dialog).getByText("Current Config")).toBeInTheDocument();
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
      name: "Apply custom to Current Config?",
    });
    const confirmation = within(dialog).getByRole("textbox");
    const confirm = within(dialog).getByRole("button", { name: "Apply" });
    expect(dialog.querySelector("dd")).toHaveTextContent("Host Tenant");
    expect(confirm).toBeDisabled();
    await user.type(confirmation, "Host Tenant");
    await user.click(confirm);
    await waitFor(() =>
      expect(applyConfig).toHaveBeenCalledWith({ kind: "host" }, "codex", "custom"),
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) =>
        Promise.resolve(configFile(target.file, applied ? "applied content" : "old content")),
      applyConfig: () => {
        applied = true;
        return Promise.resolve();
      },
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "old content",
    );
    await user.click(
      screen.getByRole("button", { name: "Apply Named Config custom to Current Config" }),
    );
    await user.click(
      within(screen.getByRole("dialog", { name: "Apply custom to Current Config?" })).getByRole(
        "button",
        { name: "Apply" },
      ),
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
    });
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
    const { api } = configApi({
      listConfigs: (tenant, agent) =>
        Promise.resolve({
          configs: [],
          files: agent === "claude" ? ["settings.json"] : ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: tenant.kind === "host" && agent === "codex",
        } satisfies ConfigListData),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
    });
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
    expect(propagate).toHaveClass(actionStyles.ghost);
    expect(propagate).not.toHaveClass(actionStyles.primarySoft);
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
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
      previewCredentialPropagation: () =>
        Promise.resolve({
          plan_id: "plan-1",
          preview: {
            updates: 2,
            entries: [
              { label: "default · Current", outcome: { status: "updated" } },
              { label: "work · team", outcome: { status: "updated" } },
              {
                label: "work · newer",
                outcome: {
                  status: "newer",
                  source_last_refresh: "2026-08-18T00:00:00Z",
                  target_last_refresh: "2026-08-19T00:00:00Z",
                },
              },
            ],
          },
        }),
      executeCredentialPropagation: () =>
        Promise.resolve({
          entries: [
            { label: "default · Current", outcome: { status: "updated" } },
            { label: "work · team", outcome: { status: "unchanged" } },
            {
              label: "work · newer",
              outcome: {
                status: "failed",
                reason: "target changed during propagation",
              },
            },
          ],
        }),
    });
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
    const { api, createConfig } = configApi({
      listConfigs: () =>
        Promise.resolve({
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
      createConfig: (_tenant, _agent, name) => {
        configs = [{ name, state: "ready" }];
        return Promise.resolve();
      },
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const create = await screen.findByRole("button", { name: "Create Named Config" });
    expect(create).toHaveClass(actionStyles.ghost);
    expect(create).not.toHaveClass(actionStyles.primary);
    await user.click(create);
    const dialog = screen.getByRole("dialog", { name: "Create Named Config" });
    const input = within(dialog).getByRole("textbox", { name: "Named Config name" });
    await user.type(input, "Bad Name");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, "new-config");
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(createConfig).toHaveBeenCalledWith(
        { kind: "managed", name: "default" },
        "codex",
        "new-config",
      ),
    );
    expect(await screen.findByRole("button", { name: "new-config" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("dialog", { name: "Create Named Config" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Apply Named Config new-config to Current Config" }),
    ).toHaveClass(actionStyles.primarySoft);
  });
  it("keeps Create disabled when the Named Config name already exists", async () => {
    const { api, createConfig } = configApi({
      listConfigs: () =>
        Promise.resolve({
          configs: [{ name: "custom", state: "ready" }],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Create Named Config" }));
    const dialog = screen.getByRole("dialog", { name: "Create Named Config" });
    const input = within(dialog).getByRole("textbox", { name: "Named Config name" });
    await user.type(input, "custom");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    expect(dialog).toHaveTextContent("Named Config custom already exists.");
    await user.keyboard("{Enter}");
    expect(createConfig).not.toHaveBeenCalled();
  });
  it("reconciles surviving selections after a non-transactional batch deletion failure", async () => {
    let configs: ConfigListData["configs"] = [
      { name: "first", state: "ready" },
      { name: "second", state: "ready" },
    ];
    const { api } = configApi({
      listConfigs: () =>
        Promise.resolve({
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData),
      revealConfigFile: (target) => Promise.resolve(configFile(target.file, "")),
      deleteConfigs: () => {
        configs = [{ name: "second", state: "ready" }];
        return Promise.reject(new Error("second Named Config could not be deleted"));
      },
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await screen.findByRole("button", { name: "first" });
    await user.click(screen.getByRole("button", { name: "Select Configs" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Named Configs" }));
    const dialog = screen.getByRole("dialog", { name: "Delete selected Named Configs?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
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
});
