import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigListData } from "@/api/configs";
import { configFile } from "@/features/configs/ConfigPage.testFixtures";
import { ConfigPage, configApi } from "@/features/configs/testSupport";
import { activeOperation } from "@/test/operations";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("ConfigPage", () => {
  it("replaces a failed Config catalog load with an error state and Retry", async () => {
    let catalogAttempts = 0;
    const catalog = {
      configs: [],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const listConfigs = vi.fn(() => {
      catalogAttempts += 1;
      return catalogAttempts === 1
        ? Promise.reject(new Error("catalog unavailable"))
        : Promise.resolve(catalog);
    });
    const { api } = configApi({ listConfigs });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
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
    const { api, revealConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) =>
        Promise.resolve({ ...configFile(target.file, ""), exists: false }),
    });
    render(<ConfigPage api={api} />);
    expect(await screen.findByText("No Named Configs found.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Current Config" })).not.toBeInTheDocument();
    expect(screen.getByText("Managed Tenant not found")).toBeInTheDocument();
    expect(screen.getByText("The selected Managed Tenant does not exist.")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "config.toml content" })).not.toBeInTheDocument();
    expect(revealConfigFile).not.toHaveBeenCalled();
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
    const { api, revealConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: (target) => {
        expect(target.current).toBe(true);
        expect(target.config).toBeNull();
        return Promise.resolve(configFile(target.file, ""));
      },
    });
    const onLocationChange = vi.fn((query: URLSearchParams, replace = false) => {
      window.history[replace ? "replaceState" : "pushState"](
        null,
        "",
        `/_aibox/ui/configs?${query}`,
      );
    });
    render(<ConfigPage api={api} onLocationChange={onLocationChange} />);
    expect(await screen.findByText("No Named Configs found.")).toBeInTheDocument();
    await waitFor(() =>
      expect(window.location.href).toContain(
        "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex",
      ),
    );
    expect(window.location.search).toBe("?tenant=managed%3Adefault&agent=codex");
    expect(screen.queryByText("Named Config missing")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(onLocationChange).toHaveBeenCalledWith(expect.any(URLSearchParams), true);
    await waitFor(() => expect(revealConfigFile).toHaveBeenCalledTimes(2));
  });
  it("retries a failed Config file reveal from the page error", async () => {
    const catalog = {
      configs: [],
      files: ["config.toml"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    let revealAttempts = 0;
    const { api, revealConfigFile } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => {
        revealAttempts += 1;
        return revealAttempts === 1
          ? Promise.reject(new Error("file unavailable"))
          : Promise.resolve(configFile("config.toml", "retried content"));
      },
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("file unavailable");
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "retried content",
    );
    expect(revealConfigFile).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("file unavailable")).not.toBeInTheDocument();
  });
  it("keeps Config browsing available but blocks writes during a Management Operation", async () => {
    const catalog = {
      configs: [{ name: "team", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const { api } = configApi({ listConfigs: () => Promise.resolve(catalog) });
    render(<ConfigPage api={api} operation={activeOperation} />);
    expect(await screen.findByRole("button", { name: "Refresh Configs" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "team" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Apply Named Config team to Current Config" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete Named Config team" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create Named Config" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Changes are temporarily unavailable");
  });
});
