import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { deferred } from "@/test/deferred";
import {
  SessionPage,
  firstSession,
  secondSession,
  list,
  fakeApi,
} from "@/features/sessions/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("aggregates every selected Tenant and Coding Agent with stable source identities", async () => {
    const streamSessionDetail = vi.fn().mockResolvedValue(undefined);
    const { api, listSessions } = fakeApi({
      sessions: (tenantSelection, agent) => {
        const tenant = tenantSelection.kind === "host" ? "host" : tenantSelection.name;
        const offsets: Record<string, string> = {
          "default:codex": "2026-08-17T09:00:00Z",
          "default:claude": "2026-08-17T07:00:00Z",
          "work:codex": "2026-08-17T08:00:00Z",
          "work:claude": "2026-08-17T10:00:00Z",
        };
        return list([
          {
            ...firstSession,
            start_ts: offsets[`${tenant}:${agent}`],
            title: `${tenant} ${agent}`,
          },
        ]);
      },
      streamSessionDetail,
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "default codex, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    let filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
    filterMenu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(
      within(filterMenu).getByRole("button", { name: "Select multiple Coding Agents" }),
    );
    await user.click(within(filterMenu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    const newest = await screen.findByRole("button", {
      name: "work claude, Tenant work · Claude",
    });
    expect(
      screen.getByRole("button", { name: "default codex, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "work codex, Tenant work · Codex" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "default claude, Tenant default · Claude" }),
    ).toBeInTheDocument();
    expect(within(newest.querySelector("small")!).getByText("work Claude")).toBeInTheDocument();
    expect(within(newest).getByText("2026-08-17 18:00:00").tagName).toBe("TIME");
    expect(newest).not.toHaveTextContent(firstSession.display_id);
    expect(listSessions).toHaveBeenCalledWith(
      { kind: "managed", name: "work" },
      expect.any(String),
      expect.any(AbortSignal),
    );
    expect(listSessions).toHaveBeenCalledWith(
      expect.any(Object),
      "claude",
      expect.any(AbortSignal),
    );
    await user.click(newest);
    expect(streamSessionDetail).toHaveBeenCalledWith(
      { kind: "managed", name: "work" },
      "claude",
      firstSession.id,
      expect.any(Object),
      expect.any(AbortSignal),
    );
    expect(
      screen
        .getAllByText(/work Claude/)
        .some((element) => element.textContent?.includes("work Claude")),
    ).toBe(true);
  });
  it("keeps readable sources but disables deletion when one source cannot be listed", async () => {
    const { api } = fakeApi({
      sessions: (tenant) => {
        if (tenant.kind === "managed" && tenant.name === "work")
          throw new Error("permission denied");
        return list([firstSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Tenant work Codex: permission denied")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "First prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Sessions" })).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
  });
  it("deletes cross-source selections serially and preserves failed survivors", async () => {
    let defaultRows = [firstSession];
    const workRows = [secondSession];
    const defaultDeletion = deferred<{
      deleted: number;
    }>();
    const deleteSessions = vi.fn((tenant: { kind: string; name?: string }) => {
      if (tenant.kind === "managed" && tenant.name === "default") {
        return defaultDeletion.promise.then((result) => {
          defaultRows = [];
          return result;
        });
      }
      return Promise.reject(new Error("work Transcript could not be deleted"));
    });
    const { api } = fakeApi({
      sessions: (tenant) =>
        list(tenant.kind === "managed" && tenant.name === "work" ? workRows : defaultRows),
      deleteSessions,
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    await screen.findByRole("button", { name: "Second prompt, Tenant work · Codex" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 2 selected Sessions?" });
    expect(dialog).toHaveTextContent("Tenant default Codex (1)");
    expect(dialog).toHaveTextContent("Tenant work Codex (1)");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(deleteSessions).toHaveBeenCalledTimes(1);
    act(() => defaultDeletion.resolve({ deleted: 1 }));
    await waitFor(() => expect(deleteSessions).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "work Transcript could not be deleted",
    );
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect Second prompt, Tenant work · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.queryByRole("button", { name: "Select First prompt, Tenant default · Codex" }),
    ).not.toBeInTheDocument();
    expect(deleteSessions.mock.calls[0]).toEqual([
      { kind: "managed", name: "default" },
      "codex",
      [firstSession.id],
    ]);
    expect(deleteSessions.mock.calls[1]).toEqual([
      { kind: "managed", name: "work" },
      "codex",
      [secondSession.id],
    ]);
  });
  it("uses two-level copy for list, detail, and empty Transcript states", async () => {
    const empty = fakeApi({ sessions: () => list([]) });
    const firstRender = render(<SessionPage api={empty.api} />);
    expect(await screen.findByText("No Sessions found")).toBeInTheDocument();
    expect(
      screen.getByText("No Sessions were found for the selected Tenants and Coding Agents."),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a Session" })).toBeInTheDocument();
    expect(
      screen.getByText("No Sessions found").closest('[data-empty-state="list"]'),
    ).toBeInTheDocument();
    expect(
      screen
        .getByRole("heading", { name: "Select a Session" })
        .closest('[data-empty-state="detail"]'),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Choose a Session to inspect its conversation and Transcript."),
    ).toBeInTheDocument();
    firstRender.unmount();
    const readable = fakeApi({ sessions: () => list([firstSession]) });
    const user = userEvent.setup();
    render(<SessionPage api={readable.api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(
      await screen.findByRole("heading", { name: "No readable conversation" }),
    ).toBeInTheDocument();
    expect(
      screen
        .getByRole("heading", { name: "No readable conversation" })
        .closest('[data-empty-state="detail"]'),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "This Transcript contains no supported user or Coding Agent messages. Transcript events remain available below when present.",
      ),
    ).toBeInTheDocument();
  });
});
