import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { TenantPage, tenantApi } from "@/features/tenants/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("TenantPage", () => {
  it("keeps Install available before checking and validates a specific version dialog", async () => {
    const { api, mutateComponent } = tenantApi({
      mutateComponent: () => Promise.resolve({ kind: "completed", value: {} }),
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

    expect(mutateComponent).toHaveBeenCalledWith(
      { kind: "managed", name: "default" },
      "python",
      true,
      "3.13.7",
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(mutateComponent).toHaveBeenCalledWith(
      { kind: "managed", name: "default" },
      "python",
      true,
      null,
    );
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
    const { api, mutateComponent } = tenantApi({
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
    expect(mutateComponent).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Remove Component" }));
    expect(mutateComponent).toHaveBeenCalledWith(
      { kind: "managed", name: "default" },
      "codex-statusline",
      false,
      null,
    );
  });
});
