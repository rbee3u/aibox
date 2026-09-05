import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ComponentRow } from "@/api/tenants";
import { TenantPage, tenantRows, tenantApi } from "@/features/tenants/testSupport";
import actionButtonStyles from "@/shared/ui/ActionButton.module.css";
import layout from "@/shared/ui/layout/catalog.module.css";
import { activeOperation } from "@/test/operations";

afterEach(() => {
  vi.useRealTimers();
  window.history.replaceState(null, "", "/");
});
describe("TenantPage", () => {
  it("offers Retry when the Tenant catalog cannot be loaded", async () => {
    let attempts = 0;
    const listTenants = vi.fn(() => {
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new Error("tenant catalog unavailable"))
        : Promise.resolve(tenantRows);
    });
    const { api } = tenantApi({ listTenants });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
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
  it("highlights a Component from the URL, then drops the query without selecting the row", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/tenants?tenant=managed%3Awork&component=rust",
    );
    const { api, listComponents } = tenantApi({
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
    expect(await screen.findByRole("heading", { name: "Components" })).toBeInTheDocument();
    expect(screen.getByLabelText("Selected Tenant: work, Managed Tenant")).toBeInTheDocument();
    const rust = await screen.findByText("Rust");
    const row = rust.closest('[role="listitem"]');
    expect(rust.closest("button")).toBeNull();
    expect(row).not.toHaveAttribute("aria-pressed");
    expect(row).toHaveAttribute("data-attention", "true");
    expect(window.location.search).toBe("?tenant=managed%3Awork");
    expect(screen.getByLabelText("Component summary")).toHaveTextContent("1/8 installed");
    expect(screen.queryByText("No issues")).not.toBeInTheDocument();
    expect(listComponents).toHaveBeenCalledWith(
      { kind: "managed", name: "work" },
      expect.any(AbortSignal),
    );
  });
  it("drops an unknown Component query without highlighting a row", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/tenants?tenant=managed%3Awork&component=nope",
    );
    const { api } = tenantApi({
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
    const rust = await screen.findByText("Rust");
    expect(rust.closest('[role="listitem"]')).not.toHaveAttribute("data-attention");
    expect(window.location.search).toBe("?tenant=managed%3Awork");
  });
  it("groups a Managed Tenant catalog without treating missing optional Components as issues", async () => {
    const components = [
      {
        kind: "codex",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
      {
        kind: "claude",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
      {
        kind: "codex-statusline",
        supports_version: false,
        status: "installed",
        version: null,
        error: null,
      },
      {
        kind: "claude-statusline",
        supports_version: false,
        status: "installed",
        version: null,
        error: null,
      },
      {
        kind: "node",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
      {
        kind: "python",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
      {
        kind: "rust",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
      {
        kind: "go",
        supports_version: true,
        status: "not-installed",
        version: null,
        error: null,
      },
    ] satisfies ComponentRow[];
    const { api } = tenantApi({ components });
    render(<TenantPage api={api} />);

    expect(await screen.findByRole("region", { name: "Coding Agents" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Statuslines" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Runtimes & Toolchains" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(8);
    for (const label of [
      "Codex",
      "Claude",
      "Codex Statusline",
      "Claude Statusline",
      "Node.js",
      "Python",
      "Rust",
      "Go",
    ]) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.queryByText("OpenAI coding agent")).not.toBeInTheDocument();
    expect(screen.queryByText("Python, uv, and pip")).not.toBeInTheDocument();
    expect(document.querySelectorAll("[data-component-icon]")).toHaveLength(8);
    for (const [component, brand] of [
      ["codex", "openai"],
      ["claude", "claude"],
      ["node", "nodejs"],
      ["python", "python"],
      ["rust", "rust"],
      ["go", "go"],
    ]) {
      const brandIcon = document.querySelector<HTMLElement>(
        `[data-component-icon="${component}"] [data-icon="${brand}"]`,
      );
      expect(brandIcon).toBeTruthy();
      expect(brandIcon?.style.getPropertyValue("--brand-icon-size")).toBe("24px");
    }
    expect(document.querySelectorAll("[data-component-icon] .lucide-activity")).toHaveLength(2);
    expect(screen.getAllByText("Not installed")).toHaveLength(6);
    expect(screen.queryByText("Definition current")).not.toBeInTheDocument();
    expect(screen.queryByText("Definition changed")).not.toBeInTheDocument();
    expect(screen.queryByText("—")).not.toBeInTheDocument();
    expect(screen.queryByText("Latest not checked")).not.toBeInTheDocument();
    expect(screen.queryByText("Ready.")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Component summary")).toHaveTextContent("2/8 installed");
    expect(screen.queryByText(/issues?/)).not.toBeInTheDocument();
  });
  it("shows only the Statuslines presentation group for the Host Tenant", async () => {
    const { api } = tenantApi({
      components: [
        {
          kind: "codex-statusline",
          supports_version: false,
          status: "not-installed",
          version: null,
          error: null,
        },
        {
          kind: "claude-statusline",
          supports_version: false,
          status: "installed",
          version: null,
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} search="?tenant=host" />);

    expect(await screen.findByRole("region", { name: "Statuslines" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Coding Agents" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Runtimes & Toolchains" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByLabelText("Selected Tenant: Host Tenant")).toBeInTheDocument();
  });
  it("keeps the newer Tenant catalog when an earlier inspection resolves late", async () => {
    let resolveDefault!: (rows: ComponentRow[]) => void;
    let resolveWork!: (rows: ComponentRow[]) => void;
    const defaultComponents = new Promise<ComponentRow[]>((resolve) => {
      resolveDefault = resolve;
    });
    const workComponents = new Promise<ComponentRow[]>((resolve) => {
      resolveWork = resolve;
    });
    const listComponents = vi.fn((tenant: { kind: string; name?: string }) => {
      if (tenant.kind === "managed" && tenant.name === "default") return defaultComponents;
      if (tenant.kind === "managed" && tenant.name === "work") return workComponents;
      return Promise.resolve([]);
    });
    const { api } = tenantApi({ listComponents });
    const user = userEvent.setup();
    render(<TenantPage api={api} search="?tenant=managed%3Adefault" />);
    expect(await screen.findByRole("progressbar", { name: "Loading Components" })).toBeVisible();

    await user.click(screen.getByRole("button", { name: "work, Managed Tenant" }));
    resolveWork([
      {
        kind: "rust",
        supports_version: true,
        status: "installed",
        version: "1.89.0",
        error: null,
      },
    ]);
    expect(await screen.findByText("Rust")).toBeInTheDocument();
    resolveDefault([
      {
        kind: "python",
        supports_version: true,
        status: "installed",
        version: "3.14.7",
        error: null,
      },
    ]);
    await waitFor(() => expect(screen.queryByText("Python")).not.toBeInTheDocument());
    expect(screen.getByText("Rust")).toBeInTheDocument();
  });
  it("restores the fallback Tenant when navigation clears the route selection", async () => {
    const { api } = tenantApi();
    const view = render(<TenantPage api={api} search="" />);

    expect(
      await screen.findByLabelText("Selected Tenant: default, Managed Tenant"),
    ).toBeInTheDocument();

    view.rerender(<TenantPage api={api} search="?tenant=host" />);
    expect(await screen.findByLabelText("Selected Tenant: Host Tenant")).toBeInTheDocument();

    view.rerender(<TenantPage api={api} search="" />);
    expect(
      await screen.findByLabelText("Selected Tenant: default, Managed Tenant"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Select a Tenant")).not.toBeInTheDocument();
  });
  it("opens the one-pane Tenant detail only when a Tenant is routed", async () => {
    const { api } = tenantApi();
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await screen.findByText("Managed Tenants");
    const catalog = screen.getByLabelText("Tenants");
    expect(catalog.parentElement).not.toHaveClass(layout.showsDetail);

    await user.click(screen.getByRole("button", { name: "default, Managed Tenant" }));
    expect(catalog.parentElement).toHaveClass(layout.showsDetail);
    expect(screen.getByRole("button", { name: "Back to Tenants" })).toBeInTheDocument();
  });
  it("groups Host and Managed Tenants and shows home paths", async () => {
    const { api } = tenantApi();
    render(<TenantPage api={api} />);
    expect(await screen.findByText("Managed Tenants")).toBeInTheDocument();
    expect(screen.getAllByText("~").length).toBeGreaterThan(0);
    expect(screen.getAllByText("~/.aibox/tenants/default").length).toBeGreaterThan(0);
    expect(screen.getByText("/var/lib/aibox/tenants/work")).toBeInTheDocument();
    expect(screen.queryByText("Default")).not.toBeInTheDocument();
    expect(screen.queryByText("Protected")).not.toBeInTheDocument();
    expect(screen.queryByText("Host risk")).not.toBeInTheDocument();
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
    expect(screen.queryByText("Protected")).not.toBeInTheDocument();
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
    const createTenant = vi.fn((name: string) => {
      rows.push({
        kind: "managed",
        name,
        display_name: name,
        home: `/home/test/.aibox/tenants/${name}`,
        exists: true,
      });
      return Promise.resolve();
    });
    const { api } = tenantApi({
      rows,
      createTenant,
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    const create = await screen.findByRole("button", { name: "Create Managed Tenant" });
    expect(create).toHaveClass(actionButtonStyles.ghost);
    expect(create).not.toHaveClass(actionButtonStyles.primary);
    await user.click(create);
    const dialog = screen.getByRole("dialog", { name: "Create Managed Tenant" });
    const input = within(dialog).getByRole("textbox", { name: "Tenant name" });
    await user.type(input, "Bad_Name");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, "new-tenant");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(createTenant).toHaveBeenCalledWith("new-tenant"));
    expect(
      await screen.findByLabelText("Selected Tenant: new-tenant, Managed Tenant"),
    ).toBeInTheDocument();
  });
  it("keeps Create disabled when the Managed Tenant name already exists", async () => {
    const createTenant = vi.fn();
    const { api } = tenantApi({ createTenant });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Create Managed Tenant" }));
    const dialog = screen.getByRole("dialog", { name: "Create Managed Tenant" });
    const input = within(dialog).getByRole("textbox", { name: "Tenant name" });
    await user.type(input, "work");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    expect(dialog).toHaveTextContent("Managed Tenant work already exists.");
    expect(createTenant).not.toHaveBeenCalled();
  });
  it("requires the Managed Tenant name for single deletion", async () => {
    const deleteTenants = vi.fn().mockResolvedValue(undefined);
    const { api } = tenantApi({ deleteTenants });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Delete Tenant work" }));
    const dialog = screen.getByRole("dialog", { name: "Delete Tenant work?" });
    const confirmation = within(dialog).getByRole("textbox");
    expect(within(dialog).getByRole("button", { name: "Delete" })).toBeDisabled();
    await user.type(confirmation, "work");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(deleteTenants).toHaveBeenCalledWith(["work"]);
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
    const deleteTenants = vi.fn(() => {
      rows = rows.filter((row) => row.name !== "work");
      return Promise.reject(new Error("extra could not be deleted"));
    });
    const listTenants = vi.fn(() => Promise.resolve(rows));
    const { api } = tenantApi({ listTenants, deleteTenants });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Select Tenants" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Tenants" }));
    const dialog = screen.getByRole("dialog", { name: "Delete selected Managed Tenants?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Deselect work" })).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Deselect extra" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
  });
});
