import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TenantPage, tenantApi } from "@/features/tenants/testSupport";
import actionButtonStyles from "@/shared/ui/ActionButton.module.css";
import { activeOperation } from "@/test/operations";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("TenantPage", () => {
  it.each([
    ["not-installed", null, "Not installed", "Install", false],
    ["installed", null, "Installed", null, true],
    ["incomplete", null, "Incomplete", "Repair", true],
    ["modified", null, "Differs", "Update", true],
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
      const statusElement = (await screen.findByText(statusLabel)).closest("[data-status-variant]");
      expect(statusElement).toHaveAttribute(
        "data-status-variant",
        status === "installed" || status === "not-installed" ? "inline" : "badge",
      );
      if (primaryAction) {
        expect(screen.getByRole("button", { name: primaryAction })).toBeEnabled();
      }
      const removeLabel = "Remove Codex Statusline";
      if (removable) expect(screen.getByRole("button", { name: removeLabel })).toBeEnabled();
      else expect(screen.queryByRole("button", { name: removeLabel })).not.toBeInTheDocument();
      const summary = screen.getByLabelText("Component summary");
      if (status === "modified") expect(summary).toHaveTextContent("1/8 installed1 differs");
    },
  );
  it("shows row-local progress until the Component Operation reaches a terminal state", async () => {
    const mutateComponent = vi
      .fn()
      .mockResolvedValue({ kind: "operation", operation: activeOperation });
    const { api } = tenantApi({
      mutateComponent,
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
    const { api, mutateComponent } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "python",
            state: "available",
            version: "3.15.0",
            newest: null,
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
    expect(within(row).getByText("Outdated")).toBeInTheDocument();
    expect(within(row).getByRole("textbox", { name: "Python version" })).toHaveValue("3.15.0");
    expect(row).not.toHaveTextContent("Update available");
    await user.click(await screen.findByRole("button", { name: "Update" }));
    expect(mutateComponent).toHaveBeenLastCalledWith(
      { kind: "managed", name: "default" },
      "python",
      true,
      "3.15.0",
    );
  });

  it("updates to an exact newer version from the inline version input", async () => {
    const { api, mutateComponent } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "python",
            state: "available",
            version: "3.15.0",
            newest: null,
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

    const row = await screen.findByRole("listitem");
    const input = within(row).getByRole("textbox", { name: "Python version" });
    const update = within(row).getByRole("button", { name: "Update" });

    expect(input).toHaveValue("3.15.0");
    expect(update).toBeEnabled();

    // Type currently installed version
    await user.clear(input);
    await user.type(input, "3.12.0");
    expect(update).toBeDisabled();

    // Type older version
    await user.clear(input);
    await user.type(input, "3.11.9");
    expect(update).toBeDisabled();

    // Type with leading 'v' to test smart prefix normalization
    await user.clear(input);
    await user.type(input, "v3.16.0");
    expect(input).toHaveValue("3.16.0");
    expect(update).toBeEnabled();

    await user.click(update);
    expect(mutateComponent).toHaveBeenLastCalledWith(
      { kind: "managed", name: "default" },
      "python",
      true,
      "3.16.0",
    );
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
              newest: null,
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
      if (latestVersion === "3.13.7") {
        expect(row).toHaveTextContent("The observed release is lower than the current version.");
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
            newest: null,
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
    expect(within(row).getByRole("textbox", { name: "Node.js version" })).toHaveValue("1.10.0");
    expect(row).not.toHaveTextContent("Update available");
    expect(screen.getByRole("button", { name: "Update" })).toBeEnabled();
  });

  it("updates an installed Node newer than LTS to the newest stable release", async () => {
    const { api, mutateComponent } = tenantApi({
      latest: {
        checked_at: "2026-08-25T08:00:00Z",
        entries: [
          {
            kind: "node",
            state: "available",
            version: "24.21.0",
            newest: "26.8.2",
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
          version: "26.1.0",
          error: null,
        },
      ],
    });
    const user = userEvent.setup();
    render(<TenantPage api={api} />);
    const row = await screen.findByRole("listitem");
    expect(row).toHaveTextContent("v26.1.0");
    expect(within(row).getByText("Outdated")).toBeInTheDocument();
    expect(within(row).getByRole("textbox", { name: "Node.js version" })).toHaveValue("26.8.2");
    await user.click(await screen.findByRole("button", { name: "Update" }));
    expect(mutateComponent).toHaveBeenLastCalledWith(
      { kind: "managed", name: "default" },
      "node",
      true,
      "26.8.2",
    );
  });

  it("combines local refresh with an explicit shared update check", async () => {
    const snapshot = {
      checked_at: "2026-08-25T08:00:00Z",
      entries: [
        {
          kind: "node" as const,
          state: "available" as const,
          version: "24.19.0",
          newest: null,
          source: "nodejs.org",
          error: null,
        },
      ],
    };
    const checkLatestComponents = vi.fn(() => Promise.resolve(snapshot));
    const { api, listComponents, latestComponents } = tenantApi({
      checkLatestComponents,
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
    const checkUpdates = await screen.findByRole("button", { name: "Check for updates" });
    expect(checkUpdates).toHaveClass(actionButtonStyles.ghost);
    expect(checkUpdates).not.toHaveClass(actionButtonStyles.secondary);
    expect(screen.getByLabelText("Component summary")).not.toHaveTextContent(/checked/i);
    await user.click(checkUpdates);
    const updatedRow = await screen.findByRole("listitem");
    await waitFor(() =>
      expect(within(updatedRow).getByRole("textbox", { name: "Node.js version" })).toHaveValue(
        "24.19.0",
      ),
    );
    /*
     * The freshness is drawn, not only announced. It used to reach the
     * accessible name alone, so a sighted reader was invited to act on the
     * Update controls below without being told how old the observation was.
     */
    expect(screen.getByText(/^Checked/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Check for updates, checked/ })).toBeInTheDocument();
    expect(checkLatestComponents).toHaveBeenCalledWith();
    expect(listComponents).toHaveBeenCalledTimes(2);

    await user.click(screen.getByRole("button", { name: "work, Managed Tenant" }));
    expect(
      await screen.findByLabelText("Selected Tenant: work, Managed Tenant"),
    ).toBeInTheDocument();
    const workRow = screen.getByRole("listitem");
    expect(within(workRow).getByRole("textbox", { name: "Node.js version" })).toHaveValue(
      "24.19.0",
    );
    expect(latestComponents).toHaveBeenCalledTimes(1);
  });

  it("renders distinct summary badges for installed, outdated, differs, and issues", async () => {
    const { api } = tenantApi({
      latest: {
        checked_at: "2026-08-16T12:00:00Z",
        entries: [
          {
            kind: "node",
            state: "available",
            version: "24.21.0",
            newest: null,
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
          version: "24.19.0",
          error: null,
        },
        {
          kind: "claude-statusline",
          supports_version: false,
          status: "modified",
          version: null,
          error: null,
        },
        {
          kind: "python",
          supports_version: true,
          status: "incomplete",
          version: null,
          error: null,
        },
      ],
    });
    render(<TenantPage api={api} />);
    const summary = await screen.findByLabelText("Component summary");
    await waitFor(() => {
      expect(summary).toHaveTextContent("2/8 installed");
      expect(summary).toHaveTextContent("1 outdated");
      expect(summary).toHaveTextContent("1 differs");
      expect(summary).toHaveTextContent("1 issue");
    });
  });
});
