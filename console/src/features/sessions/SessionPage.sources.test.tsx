import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionPage, firstSession, list, fakeApi } from "@/features/sessions/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("switches Tenant and Agent and inspects from the selected source", async () => {
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
    await screen.findByRole("button", { name: "default codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(tenantMenu).getByRole("option", { name: "work" }));
    await screen.findByRole("button", { name: "work codex" });
    await user.click(screen.getByRole("button", { name: "Agent: Codex" }));
    const agentMenu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(agentMenu).getByRole("option", { name: "Claude" }));
    const newest = await screen.findByRole("button", {
      name: "work claude",
    });
    expect(within(newest).getByText("2026-08-17 18:00:00").tagName).toBe("TIME");
    expect(newest).not.toHaveTextContent(firstSession.display_id);
    expect(listSessions).toHaveBeenCalledWith(
      { kind: "managed", name: "work" },
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
      screen.getAllByText(/work/).some((element) => element.textContent?.includes("work")),
    ).toBe(true);
  });
  it("shows error when the selected source cannot be listed", async () => {
    const { api } = fakeApi({
      sessions: (tenant) => {
        if (tenant.kind === "managed" && tenant.name === "work")
          throw new Error("permission denied");
        return list([firstSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("option", { name: "work" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("permission denied");
    expect(screen.queryByRole("button", { name: "First prompt" })).not.toBeInTheDocument();
  });
  it("uses two-level copy for list, detail, and empty Transcript states", async () => {
    const empty = fakeApi({ sessions: () => list([]) });
    const firstRender = render(<SessionPage api={empty.api} />);
    expect(await screen.findByText("No Sessions found")).toBeInTheDocument();
    expect(
      screen.getByText("No Sessions were found for the selected Tenant and Agent."),
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
    await user.click(await screen.findByRole("button", { name: "First prompt" }));
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
        "This Transcript contains no supported user or Agent messages. Transcript events stay on Details.",
      ),
    ).toBeInTheDocument();
    // The rail stays mounted with nothing to number, so the reading keeps its column.
    expect(screen.getAllByRole("navigation", { name: "Conversation messages" })).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /Jump to message/ })).not.toBeInTheDocument();
  });
});
