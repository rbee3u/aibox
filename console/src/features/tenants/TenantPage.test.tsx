import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TenantPage, tenantRows, activeOperation, tenantApi } from "@/test/managementTestSupport";

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
  it("opens the Tenant from a historical Component URL and drops the Component selection", async () => {
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
    expect(await screen.findByRole("heading", { name: "Components" })).toBeInTheDocument();
    expect(screen.getByLabelText("Selected Tenant: work, Managed Tenant")).toBeInTheDocument();
    const rust = await screen.findByText("Rust");
    expect(rust.closest("button")).toBeNull();
    expect(rust.closest('[role="listitem"]')).not.toHaveAttribute("aria-pressed");
    expect(window.location.search).toBe("?tenant=managed%3Awork");
    expect(screen.getByLabelText("Component summary")).toHaveTextContent("1/8 installed");
    expect(screen.queryByText("No issues")).not.toBeInTheDocument();
    expect(get).toHaveBeenCalledWith("/_aibox/api/components?tenant=managed%3Awork", undefined);
  });
  it("groups a Managed Tenant catalog without treating missing optional Components as issues", async () => {
    const components = [
      ["codex", true, "not-installed", null],
      ["claude", true, "not-installed", null],
      ["codex-statusline", false, "installed", null],
      ["claude-statusline", false, "installed", null],
      ["node", true, "not-installed", null],
      ["python", true, "not-installed", null],
      ["rust", true, "not-installed", null],
      ["go", true, "not-installed", null],
    ].map(([kind, supports_version, status, version]) => ({
      kind: String(kind),
      supports_version: Boolean(supports_version),
      status: String(status),
      version: version ? String(version) : null,
      error: null,
    }));
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
    let resolveDefault!: (rows: unknown[]) => void;
    let resolveWork!: (rows: unknown[]) => void;
    const defaultComponents = new Promise<unknown[]>((resolve) => {
      resolveDefault = resolve;
    });
    const workComponents = new Promise<unknown[]>((resolve) => {
      resolveWork = resolve;
    });
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenantRows);
      if (path === "/_aibox/api/components/latest") return Promise.resolve(null);
      if (path === "/_aibox/api/components?tenant=managed%3Adefault") {
        return defaultComponents;
      }
      if (path === "/_aibox/api/components?tenant=managed%3Awork") return workComponents;
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const user = userEvent.setup();
    render(<TenantPage api={{ get, post: vi.fn() }} search="?tenant=managed%3Adefault" />);
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
    expect(
      await screen.findByLabelText("Selected Tenant: new-tenant, Managed Tenant"),
    ).toBeInTheDocument();
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
    ["modified", null, "Modified", "Update", true],
    ["unmanaged", null, "Unmanaged", null, false],
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
      const removeLabel = "Remove Codex Statusline";
      if (removable) expect(screen.getByRole("button", { name: removeLabel })).toBeEnabled();
      else expect(screen.queryByRole("button", { name: removeLabel })).not.toBeInTheDocument();
      const summary = screen.getByLabelText("Component summary");
      if (status === "modified") expect(summary).toHaveTextContent("1/8 installed1 issue");
    },
  );
  it("shows row-local progress until the Component Operation reaches a terminal state", async () => {
    const post = vi.fn().mockResolvedValue(activeOperation);
    const { api } = tenantApi({
      post,
      components: [
        {
          kind: "python",
          supports_version: true,
          status: "not-installed",
          version: null,
          error: null,
        },
      ],
    });
    const onOperation = vi.fn();
    const user = userEvent.setup();
    const view = render(<TenantPage api={api} onOperation={onOperation} />);

    await user.click(await screen.findByRole("button", { name: "Install" }));
    expect(onOperation).toHaveBeenCalledWith(activeOperation);
    expect(screen.getByRole("status")).toHaveTextContent("Installing…");
    expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();

    view.rerender(
      <TenantPage
        api={api}
        onOperation={onOperation}
        operation={{
          ...activeOperation,
          state: "succeeded",
          ended_at: "2026-08-19T01:01:00Z",
        }}
      />,
    );
    expect(await screen.findByRole("button", { name: "Install" })).toBeEnabled();
    expect(screen.queryByText("Installing…")).not.toBeInTheDocument();
  });
  it("offers Update only when the checked stable release is newer", async () => {
    const post = vi.fn().mockResolvedValue({});
    const { api } = tenantApi({
      post,
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "python",
            state: "available",
            version: "3.15.0",
            source: "python.org",
            error: null,
          },
        ],
      },
      components: [
        {
          kind: "python",
          supports_version: true,
          status: "installed",
          version: "3.14.7",
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);

    const row = await screen.findByRole("listitem");
    expect(row).toHaveTextContent("v3.14.7");
    expect(row).toHaveTextContent("Latest v3.15.0");
    expect(row).not.toHaveTextContent("Update available");
    await user.click(await screen.findByRole("button", { name: "Update" }));
    expect(post).toHaveBeenLastCalledWith("/_aibox/api/components/install", {
      tenant: "managed:default",
      component: "python",
      version: "3.15.0",
    });
  });

  it("updates to an exact newer version from the split action", async () => {
    const post = vi.fn().mockResolvedValue({});
    const { api } = tenantApi({
      post,
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "python",
            state: "available",
            version: "3.15.0",
            source: "python.org",
            error: null,
          },
        ],
      },
      components: [
        {
          kind: "python",
          supports_version: true,
          status: "installed",
          version: "3.12.0",
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);

    const options = await screen.findByRole("button", {
      name: "Update options for Python",
    });
    await user.click(options);
    const menuItem = screen.getByRole("menuitem", { name: "Update to version…" });
    await waitFor(() => expect(menuItem).toHaveFocus());
    await user.keyboard("{Escape}");
    await waitFor(() => expect(options).toHaveFocus());
    await user.click(options);
    await user.click(screen.getByRole("menuitem", { name: "Update to version…" }));

    const dialog = screen.getByRole("dialog", {
      name: "Update Python version",
    });
    expect(within(dialog).getByText("Enter a stable version newer than v3.12.0.")).toBeVisible();
    const version = within(dialog).getByRole("textbox", { name: "Component version" });
    const update = within(dialog).getByRole("button", { name: "Update version" });

    await user.type(version, "3.12.0");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("already installed");
    expect(update).toBeDisabled();

    await user.clear(version);
    await user.type(version, "3.11.9");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Remove the Component");
    expect(update).toBeDisabled();

    await user.clear(version);
    await user.type(version, "3.16.0");
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
    await user.click(update);
    expect(post).toHaveBeenLastCalledWith("/_aibox/api/components/install", {
      tenant: "managed:default",
      component: "python",
      version: "3.16.0",
    });
  });

  it.each(["3.14.7", "3.13.7"])(
    "does not offer Update when latest release is %s",
    async (latestVersion) => {
      const { api } = tenantApi({
        latest: {
          checked_at: "2026-08-25T08:00:00Z",
          entries: [
            {
              kind: "python",
              state: "available",
              version: latestVersion,
              source: "python.org",
              error: null,
            },
          ],
        },
        components: [
          {
            kind: "python",
            supports_version: true,
            status: "installed",
            version: "3.14.7",
            error: null,
          },
        ],
      });
      render(<TenantPage api={api} />);
      const row = await screen.findByRole("listitem");
      expect(row).toHaveTextContent("v3.14.7");
      if (latestVersion === "3.14.7") {
        expect(row).not.toHaveTextContent("Latest");
      } else {
        expect(row).toHaveTextContent(`Latest v${latestVersion}`);
      }
      expect(row).not.toHaveTextContent("Current");
      expect(screen.queryByRole("button", { name: "Update" })).not.toBeInTheDocument();
    },
  );

  it("compares stable versions numerically", async () => {
    const { api } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "node",
            state: "available",
            version: "1.10.0",
            source: "nodejs.org",
            error: null,
          },
        ],
      },
      components: [
        {
          kind: "node",
          supports_version: true,
          status: "installed",
          version: "1.9.0",
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} />);
    const row = await screen.findByRole("listitem");
    expect(row).toHaveTextContent("v1.9.0");
    expect(row).toHaveTextContent("Latest v1.10.0");
    expect(row).not.toHaveTextContent("Update available");
    expect(screen.getByRole("button", { name: "Update" })).toBeEnabled();
  });

  it("combines local refresh with an explicit shared update check", async () => {
    const snapshot = {
      checked_at: "2026-08-25T08:00:00Z",
      entries: [
        {
          kind: "node" as const,
          state: "available" as const,
          version: "24.19.0",
          source: "nodejs.org",
          error: null,
        },
      ],
    };
    const post = vi.fn((path: string) => {
      if (path === "/_aibox/api/components/latest/check") return Promise.resolve(snapshot);
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const { api, get } = tenantApi({
      post,
      components: [
        {
          kind: "node",
          supports_version: true,
          status: "not-installed",
          version: null,
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByRole("listitem")).toHaveTextContent("Latest v24.19.0");
    expect(screen.getByText(/Checked/)).toBeInTheDocument();
    expect(post).toHaveBeenCalledWith("/_aibox/api/components/latest/check", {});
    expect(
      get.mock.calls.filter(([path]) => String(path).startsWith("/_aibox/api/components?")),
    ).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "work, Managed Tenant" }));
    expect(
      await screen.findByLabelText("Selected Tenant: work, Managed Tenant"),
    ).toBeInTheDocument();
    expect(screen.getByRole("listitem")).toHaveTextContent("Latest v24.19.0");
    expect(
      get.mock.calls.filter(([path]) => path === "/_aibox/api/components/latest"),
    ).toHaveLength(1);
  });

  it("keeps Install available before checking and validates a specific version dialog", async () => {
    const post = vi.fn().mockResolvedValue({});
    const { api } = tenantApi({
      post,
      components: [
        {
          kind: "python",
          supports_version: true,
          status: "not-installed",
          version: null,
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);

    const row = await screen.findByRole("listitem");
    expect(screen.getByText("Not checked")).toBeInTheDocument();
    expect(within(row).queryByLabelText("Python latest release")).toBeNull();
    expect(row).not.toHaveTextContent("—");
    const options = screen.getByRole("button", {
      name: "Install options for Python",
    });
    await user.click(options);
    const menuItem = screen.getByRole("menuitem", { name: "Install version…" });
    await waitFor(() => expect(menuItem).toHaveFocus());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
    await waitFor(() => expect(options).toHaveFocus());
    await user.click(options);
    await user.click(screen.getByRole("menuitem", { name: "Install version…" }));
    const dialog = screen.getByRole("dialog", {
      name: "Install Python version",
    });
    expect(within(dialog).getByText("Enter a stable version in X.Y.Z form.")).toBeVisible();
    const version = within(dialog).getByRole("textbox", { name: "Component version" });
    await user.type(version, "3.13");
    expect(within(dialog).getByRole("button", { name: "Install version" })).toBeDisabled();
    await user.clear(version);
    await user.type(version, "3.13.7");
    await user.click(within(dialog).getByRole("button", { name: "Install version" }));

    expect(post).toHaveBeenCalledWith("/_aibox/api/components/install", {
      tenant: "managed:default",
      component: "python",
      version: "3.13.7",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(post).toHaveBeenCalledWith("/_aibox/api/components/install", {
      tenant: "managed:default",
      component: "python",
      version: null,
    });
  });

  it("shows unavailable release observations without an Update action", async () => {
    const { api } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "codex",
            state: "unavailable",
            version: null,
            source: "chatgpt.com",
            error: "No comparable stable release feed.",
          },
        ],
      },
      components: [
        {
          kind: "codex",
          supports_version: true,
          status: "installed",
          version: "1.2.3",
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} />);
    const row = await screen.findByRole("listitem");
    expect(row).toHaveTextContent("Latest unavailable");
    expect(within(row).getByText("Latest unavailable")).not.toHaveAttribute("title");
    const user = userEvent.setup();
    const details = within(row).getByRole("button", { name: "Details" });
    expect(details).toHaveAttribute("aria-expanded", "false");
    await user.click(details);
    expect(details).toHaveAttribute("aria-expanded", "true");
    expect(row).toHaveTextContent("No comparable stable release feed.");
    await user.click(details);
    expect(details).toHaveAttribute("aria-expanded", "false");
    expect(row).not.toHaveTextContent("No comparable stable release feed.");
    expect(screen.queryByRole("button", { name: "Update" })).not.toBeInTheDocument();
  });
  it("shows incomparable versions as an inline diagnostic", async () => {
    const { api } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "node",
            state: "available",
            version: "24.19.0",
            source: "nodejs.org",
            error: null,
          },
        ],
      },
      components: [
        {
          kind: "node",
          supports_version: true,
          status: "installed",
          version: "development",
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    const row = await screen.findByRole("listitem");
    expect(row).toHaveTextContent("vdevelopment");
    expect(row).toHaveTextContent("Latest v24.19.0");
    await user.click(within(row).getByRole("button", { name: "Details" }));
    expect(row).toHaveTextContent("The observed and current versions could not be compared.");
    expect(screen.queryByRole("button", { name: "Update" })).not.toBeInTheDocument();
  });
  it("does not compare an incomplete Component with the latest release", async () => {
    const { api } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "python",
            state: "available",
            version: "3.15.0",
            source: "github.com/astral-sh/python-build-standalone",
            error: null,
          },
        ],
      },
      components: [
        {
          kind: "python",
          supports_version: true,
          status: "incomplete",
          version: "3.14.7",
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} />);
    expect(await screen.findByRole("listitem")).toHaveTextContent("Latest v3.15.0");
    expect(screen.getByRole("button", { name: "Repair" })).toBeEnabled();
    expect(screen.queryByText("Installed version 3.14.7 is older.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update" })).not.toBeInTheDocument();
  });
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
    await user.click(await screen.findByRole("button", { name: "Remove Codex Statusline" }));
    const dialog = screen.getByRole("dialog", { name: "Remove Codex Statusline?" });
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
