import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TenantPage, tenantRows, activeOperation, tenantApi } from "./managementTestSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("TenantPage", () => {
  it("offers Retry when the Tenant catalog cannot be loaded", async () => {
    let attempts = 0;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") {
        attempts += 1;
        return attempts === 1
          ? Promise.reject(new Error("tenant catalog unavailable"))
          : Promise.resolve(tenantRows);
      }
      if (path.startsWith("/_aibox/api/components?")) return Promise.resolve([]);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const user = userEvent.setup();
    render(<TenantPage api={{ get, post: vi.fn() }} />);
    const alert = await screen.findByRole("alert");
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Managed Tenants")).toBeInTheDocument();
    expect(screen.queryByText("tenant catalog unavailable")).not.toBeInTheDocument();
  });
  it("keeps browsing available but blocks mutations during a Management Operation", async () => {
    const { api } = tenantApi();
    render(<TenantPage api={api} operation={activeOperation} />);
    expect(await screen.findByRole("button", { name: "Refresh Tenants" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Select Tenants" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Create Managed Tenant" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Changes are temporarily unavailable");
  });
  it("restores a Tenant and selected Component from the shareable URL", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/tenants?tenant=managed%3Awork&component=rust",
    );
    const { api, get } = tenantApi({
      components: [
        {
          kind: "rust",
          supports_version: true,
          status: "installed",
          version: "1.89.0",
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} />);
    expect(await screen.findByRole("heading", { name: "work" })).toBeInTheDocument();
    const rust = await screen.findByText("Rust toolchain");
    expect(rust.closest("button")).toHaveAttribute("aria-pressed", "true");
    expect(get).toHaveBeenCalledWith("/_aibox/api/components?tenant=managed%3Awork", undefined);
  });
  it("restores the fallback Tenant when navigation clears the route selection", async () => {
    const { api } = tenantApi();
    const view = render(<TenantPage api={api} search="" />);

    expect(await screen.findByRole("heading", { name: "default" })).toBeInTheDocument();

    view.rerender(<TenantPage api={api} search="?tenant=host" />);
    expect(await screen.findByRole("heading", { name: "Host Tenant" })).toBeInTheDocument();

    view.rerender(<TenantPage api={api} search="" />);
    expect(await screen.findByRole("heading", { name: "default" })).toBeInTheDocument();
    expect(screen.queryByText("Select a Tenant")).not.toBeInTheDocument();
  });
  it("groups Host and Managed Tenants and shows home paths", async () => {
    const { api } = tenantApi();
    render(<TenantPage api={api} />);
    expect(await screen.findByText("Managed Tenants")).toBeInTheDocument();
    expect(screen.getByText(/Console-only · ~/)).toBeInTheDocument();
    expect(screen.getByText(/Managed Tenant · ~\/\.aibox\/tenants\/default/)).toBeInTheDocument();
    expect(
      screen.getByText(/Managed Tenant · \/var\/lib\/aibox\/tenants\/work/),
    ).toBeInTheDocument();
    expect(screen.getByText("Default")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh Tenants" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Tenants" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Host Tenant" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });
  it("protects Host from bulk selection and disables create in selection mode", async () => {
    const { api } = tenantApi();
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Select Tenants" }));
    const host = screen.getByRole("button", { name: "Host Tenant cannot be selected" });
    expect(host).toBeDisabled();
    expect(screen.getAllByText("Protected")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Create Managed Tenant" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Default Managed Tenant is protected and cannot be selected",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deselect work" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
  it("creates a DNS-label Tenant from the plus dialog and opens its detail", async () => {
    const rows = [...tenantRows];
    const post = vi.fn(
      (
        path: string,
        body: {
          name?: string;
        },
      ) => {
        if (path === "/_aibox/api/tenants") {
          rows.push({
            kind: "managed",
            name: body.name ?? "",
            display_name: body.name ?? "",
            home: `/home/test/.aibox/tenants/${body.name ?? ""}`,
            exists: true,
          });
          return Promise.resolve({ created: body.name });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const { api } = tenantApi({
      rows,
      post,
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Create Managed Tenant" }));
    const dialog = screen.getByRole("dialog", { name: "Create Managed Tenant" });
    const input = within(dialog).getByRole("textbox", { name: "Tenant name" });
    await user.type(input, "Bad_Name");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, "new-tenant");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/tenants", { name: "new-tenant" }),
    );
    expect(await screen.findByRole("heading", { name: "new-tenant" })).toBeInTheDocument();
  });
  it("requires the Managed Tenant name for single deletion", async () => {
    const post = vi.fn().mockResolvedValue({ deleted: 1 });
    const { api } = tenantApi({ post });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Delete Tenant work" }));
    const dialog = screen.getByRole("dialog", { name: "Delete Tenant work?" });
    const confirmation = within(dialog).getByRole("textbox");
    expect(within(dialog).getByRole("button", { name: "Delete Tenant" })).toBeDisabled();
    await user.type(confirmation, "work");
    await user.click(within(dialog).getByRole("button", { name: "Delete Tenant" }));
    expect(post).toHaveBeenCalledWith("/_aibox/api/tenants/delete", {
      names: ["work"],
      all: false,
      confirmation: "work",
    });
  });
  it("keeps surviving selections after a partial batch deletion failure", async () => {
    let rows = [
      ...tenantRows,
      {
        kind: "managed" as const,
        name: "extra",
        display_name: "extra",
        home: "/var/lib/aibox/tenants/extra",
        exists: true,
      },
    ];
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants/delete") {
        rows = rows.filter((row) => row.name !== "work");
        return Promise.reject(new Error("extra could not be deleted"));
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(rows);
      if (path.startsWith("/_aibox/api/components?")) return Promise.resolve([]);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    };
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Select Tenants" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Tenants" }));
    const dialog = screen.getByRole("dialog", { name: "Delete selected Managed Tenants?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete selected" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Deselect work" })).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Deselect extra" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
  });
  it.each([
    ["not-installed", null, "Not installed", "Install", false],
    ["installed", null, "Installed", null, true],
    ["incomplete", null, "Incomplete", "Repair", true],
    ["modified", null, "Modified", "Restore", true],
    ["unmanaged", null, "Unmanaged", null, true],
    [null, "unsafe component state", "Inspection error", "Retry inspection", false],
  ] as const)(
    "maps Component status %s to its safe action set",
    async (status, error, statusLabel, primaryAction, removable) => {
      const { api } = tenantApi({
        components: [
          {
            kind: "codex-statusline",
            supports_version: false,
            status,
            version: null,
            error,
          },
        ],
      });
      render(<TenantPage api={api} />);
      expect(await screen.findByText(statusLabel)).toBeInTheDocument();
      if (primaryAction) {
        expect(screen.getByRole("button", { name: primaryAction })).toBeEnabled();
      }
      const removeLabel = status === "unmanaged" ? "Remove detected state" : "Remove";
      if (removable) expect(screen.getByRole("button", { name: removeLabel })).toBeEnabled();
      else expect(screen.queryByRole("button", { name: removeLabel })).not.toBeInTheDocument();
    },
  );
  it("summarizes Component removal and waits for confirmation", async () => {
    const post = vi.fn().mockResolvedValue({});
    const { api } = tenantApi({
      post,
      components: [
        {
          kind: "codex-statusline",
          supports_version: false,
          status: "modified",
          version: null,
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    const dialog = screen.getByRole("dialog", { name: "Remove Codex status line?" });
    expect(dialog).toHaveTextContent("Tenant: default");
    expect(dialog).toHaveTextContent("Current state: Modified");
    expect(post).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Remove Component" }));
    expect(post).toHaveBeenCalledWith("/_aibox/api/components/remove", {
      tenant: "managed:default",
      component: "codex-statusline",
      version: null,
    });
  });
});
