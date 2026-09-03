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
      if (status === "modified") expect(summary).toHaveTextContent("1/8 installed1 issue");
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
    expect(mutateComponent).toHaveBeenLastCalledWith(
      { kind: "managed", name: "default" },
      "python",
      true,
      "3.15.0",
    );
  });

  it("updates to an exact newer version from the split action", async () => {
    const { api, mutateComponent } = tenantApi({
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
    await user.click(checkUpdates);
    expect(await screen.findByRole("listitem")).toHaveTextContent("Latest v24.19.0");
    expect(screen.getByText(/Checked/)).toBeInTheDocument();
    expect(checkLatestComponents).toHaveBeenCalledWith();
    expect(listComponents).toHaveBeenCalledTimes(2);

    await user.click(screen.getByRole("button", { name: "work, Managed Tenant" }));
    expect(
      await screen.findByLabelText("Selected Tenant: work, Managed Tenant"),
    ).toBeInTheDocument();
    expect(screen.getByRole("listitem")).toHaveTextContent("Latest v24.19.0");
    expect(latestComponents).toHaveBeenCalledTimes(1);
  });
});
