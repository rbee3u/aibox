import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { deferred } from "@/test/deferred";
import {
  SessionPage,
  firstSession,
  secondSession,
  thirdSession,
  list,
  fakeApi,
} from "@/features/sessions/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("confirms one Session deletion, aborts its detail stream, and restores list focus", async () => {
    let rows = [firstSession, secondSession];
    const deletion = deferred<{
      deleted: number;
    }>();
    let detailSignal: AbortSignal | undefined;
    const deleteSessions = vi.fn(() => deletion.promise);
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, _handlers, signal?: AbortSignal) => {
      detailSignal = signal;
      return new Promise<void>((_resolve, reject) => {
        signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      });
    });
    const { api } = fakeApi({ sessions: () => list(rows), deleteSessions, streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(detailSignal).toBeDefined();
    await user.click(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "Delete Session 111111111111?" });
    expect(dialog).toHaveTextContent(
      "Permanently deletes its Transcript from Tenant default Codex.",
    );
    expect(detailSignal?.aborted).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(detailSignal?.aborted).toBe(true);
    expect(
      screen.getByRole("button", {
        name: "Deleting Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeDisabled();
    expect(deleteSessions).toHaveBeenCalledWith({ kind: "managed", name: "default" }, "codex", [
      firstSession.id,
    ]);
    rows = [secondSession];
    act(() => deletion.resolve({ deleted: 1 }));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "First prompt, Tenant default · Codex" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Select a Session")).toBeInTheDocument();
    expect(document.querySelector('[data-icon="session-empty"]')).toHaveClass(
      "lucide-messages-square",
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "Delete Session 222222222222 from Tenant default · Codex",
        }),
      ).toHaveFocus(),
    );
  });
  it("selects the loaded snapshot and confirms deletion of only those explicit IDs", async () => {
    let rows = [firstSession, secondSession];
    const deleteSessions = vi.fn().mockResolvedValue({ deleted: 2 });
    const { api } = fakeApi({ sessions: () => list(rows), deleteSessions });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const count = screen.getByText("0 selected");
    const selectAll = screen.getByRole("button", { name: "Select all" });
    const deleteSelected = screen.getByRole("button", { name: "Delete selected Sessions" });
    expect(cancel.compareDocumentPosition(count) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      count.compareDocumentPosition(selectAll) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      selectAll.compareDocumentPosition(deleteSelected) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await user.click(cancel);
    expect(screen.getByRole("button", { name: "Select Sessions" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect First prompt, Tenant default · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 2 selected Sessions?" });
    expect(dialog).toHaveTextContent("Sources: Tenant default Codex (2)");
    rows = [thirdSession];
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(deleteSessions).toHaveBeenCalledWith({ kind: "managed", name: "default" }, "codex", [
        firstSession.id,
        secondSession.id,
      ]),
    );
    expect(
      await screen.findByRole("button", { name: "New prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Sessions" })).toBeInTheDocument();
  });
  it("reconciles surviving selections after a non-transactional batch failure", async () => {
    let rows = [firstSession, secondSession];
    const deleteSessions = vi.fn().mockImplementation(() => {
      rows = [secondSession];
      return Promise.reject(new Error("second Transcript could not be deleted"));
    });
    const { api } = fakeApi({ sessions: () => list(rows), deleteSessions });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("second Transcript could not be deleted");
    expect(within(alert).queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect Second prompt, Tenant default · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Select Sessions" })).not.toBeInTheDocument();
  });
  it("disables deletion for an incomplete view but not for Transcript content warnings", async () => {
    const warnedSession = {
      ...firstSession,
      warnings: ["skipped 1 malformed JSONL record(s)"],
    };
    const incomplete = fakeApi({
      sessions: () => list([warnedSession], ["walk session directory: permission denied"]),
    });
    const firstRender = render(<SessionPage api={incomplete.api} />);
    expect(await screen.findByRole("button", { name: "Select Sessions" })).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
    firstRender.unmount();
    const readable = fakeApi({ sessions: () => list([warnedSession]) });
    render(<SessionPage api={readable.api} />);
    expect(await screen.findByRole("button", { name: "Select Sessions" })).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeEnabled();
  });
  it("names the real Host Home in the selected deletion confirmation", async () => {
    const { api, deleteSessions } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(await screen.findByRole("button", { name: "Tenant: default" }));
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    const hostSession = await screen.findByRole("button", {
      name: "First prompt, Host Tenant · Codex",
    });
    expect(hostSession.querySelector("small")).toHaveTextContent("Host Tenant Codex");
    expect(within(hostSession).getByText("2026-08-17 17:00:00").tagName).toBe("TIME");
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Sources: Host Tenant Codex (2)");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(deleteSessions).not.toHaveBeenCalled();
  });
});
