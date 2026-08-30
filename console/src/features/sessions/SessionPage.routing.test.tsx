import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  SessionPage,
  firstSession,
  secondSession,
  thirdSession,
  list,
  fakeApi,
  completeSessionDetail,
} from "@/features/sessions/testSupport";
import { activeOperation } from "@/test/operations";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("offers a local Retry when the initial Session list fails", async () => {
    let attempts = 0;
    const { api } = fakeApi({
      sessions: () => {
        attempts += 1;
        if (attempts === 1) return Promise.reject(new Error("catalog unavailable"));
        return list([firstSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t load Sessions");
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
  });
  it("keeps Session browsing available but blocks deletion during a Management Operation", async () => {
    const { api } = fakeApi({ sessions: () => list([firstSession]) });
    render(<SessionPage api={api} operation={activeOperation} />);
    expect(await screen.findByRole("button", { name: "Refresh Sessions" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "First prompt, Tenant default · Codex" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Changes are temporarily unavailable");
  });
  it("restores repeated filters and a uniquely sourced Session from the URL", async () => {
    window.history.replaceState(
      null,
      "",
      `/_aibox/ui/sessions?tenant=managed%3Adefault&tenant=host&agent=codex&agent=claude&session_tenant=host&session_agent=claude&session=${firstSession.id}`,
    );
    const { api, listSessions, streamSessionDetail } = fakeApi({
      sessions: () => list([firstSession]),
      streamSessionDetail: completeSessionDetail,
    });
    render(<SessionPage api={api} />);
    expect(await screen.findByRole("heading", { name: "First prompt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tenant: 2 tenants" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Coding Agent: 2 Coding Agents" }),
    ).toBeInTheDocument();
    expect(listSessions).toHaveBeenCalledTimes(4);
    expect(streamSessionDetail).toHaveBeenCalledWith(
      { kind: "host" },
      "claude",
      firstSession.id,
      expect.any(Object),
      expect.any(AbortSignal),
    );
  });
  it("restores the Details tab and keeps Session deletion in the catalog", async () => {
    window.history.replaceState(
      null,
      "",
      `/_aibox/ui/sessions?tenant=managed%3Adefault&agent=codex&session_tenant=managed%3Adefault&session_agent=codex&session=${firstSession.id}&tab=details`,
    );
    const { api } = fakeApi({
      sessions: () => list([firstSession]),
      streamSessionDetail: completeSessionDetail,
    });
    render(<SessionPage api={api} />);
    expect(await screen.findByRole("heading", { name: "First prompt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Details" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("heading", { name: "Session" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Diagnostics" })).toBeInTheDocument();
    expect(screen.getByText("0ms", { exact: true })).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toHaveLength(1);
  });
  it("does not navigate when clicking the already active Session view", async () => {
    const { api } = fakeApi({
      sessions: () => list([firstSession]),
      streamSessionDetail: completeSessionDetail,
    });
    const onLocationChange = vi.fn();
    const user = userEvent.setup();
    render(<SessionPage api={api} onLocationChange={onLocationChange} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(await screen.findByRole("button", { name: "Conversation" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    onLocationChange.mockClear();
    await user.click(screen.getByRole("button", { name: "Conversation" }));
    expect(onLocationChange).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Details" }));
    expect(onLocationChange).toHaveBeenCalledTimes(1);
    onLocationChange.mockClear();
    await user.click(screen.getByRole("button", { name: "Details" }));
    expect(onLocationChange).not.toHaveBeenCalled();
  });
  it("keeps the Session catalog mounted when synchronizing an internal selection URL", async () => {
    const { api, listSessions, streamSessionDetail } = fakeApi({
      sessions: () => list([firstSession, secondSession, thirdSession]),
      streamSessionDetail: completeSessionDetail,
    });
    const onLocationChange = vi.fn();
    const user = userEvent.setup();
    const view = render(<SessionPage api={api} search="" onLocationChange={onLocationChange} />);
    const row = await screen.findByRole("button", {
      name: "Second prompt, Tenant default · Codex",
    });
    const catalog = row.parentElement?.parentElement as HTMLDivElement;
    catalog.scrollTop = 480;

    await user.click(row);
    await screen.findByRole("heading", { name: "Second prompt" });
    const query = onLocationChange.mock.calls[0][0] as URLSearchParams;
    view.rerender(
      <SessionPage api={api} search={`?${query.toString()}`} onLocationChange={onLocationChange} />,
    );
    await act(async () => Promise.resolve());

    expect(catalog).toBe(row.parentElement?.parentElement);
    expect(catalog.scrollTop).toBe(480);
    expect(listSessions).toHaveBeenCalledTimes(1);
    expect(streamSessionDetail).toHaveBeenCalledTimes(1);

    view.rerender(<SessionPage api={api} search="" onLocationChange={onLocationChange} />);
    expect(await screen.findByRole("heading", { name: "Select a Session" })).toBeInTheDocument();
    await waitFor(() => expect(listSessions).toHaveBeenCalledTimes(2));
  });
});
