import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ControlApi,
  SessionDetailMeta,
  SessionDetailStats,
  SessionListData,
} from "./controlApi";
import { deferred } from "./test/fixtures";
import {
  SessionPage,
  firstSession,
  secondSession,
  thirdSession,
  activeOperation,
  list,
  fakeApi,
  sessionQuery,
} from "./managementTestSupport";

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
    const { api, get, streamSessionDetail } = fakeApi({
      sessions: () => list([firstSession]),
    });
    render(<SessionPage api={api} />);
    expect(await screen.findByRole("heading", { name: "First prompt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tenant: 2 tenants" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Coding Agent: 2 Coding Agents" }),
    ).toBeInTheDocument();
    const sessionCalls = get.mock.calls.filter(([path]) =>
      String(path).startsWith("/_aibox/api/sessions?"),
    );
    expect(sessionCalls).toHaveLength(4);
    expect(streamSessionDetail).toHaveBeenCalledWith(
      `/_aibox/api/sessions/detail?tenant=host&agent=claude&id=${firstSession.id}`,
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
    const { api } = fakeApi({ sessions: () => list([firstSession]) });
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
    const { api } = fakeApi({ sessions: () => list([firstSession]) });
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
    const { api, get, streamSessionDetail } = fakeApi({
      sessions: () => list([firstSession, secondSession, thirdSession]),
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
    const query = onLocationChange.mock.calls[0][1] as URLSearchParams;
    view.rerender(
      <SessionPage api={api} search={`?${query.toString()}`} onLocationChange={onLocationChange} />,
    );
    await act(async () => Promise.resolve());

    expect(catalog).toBe(row.parentElement?.parentElement);
    expect(catalog.scrollTop).toBe(480);
    expect(
      get.mock.calls.filter(([path]) => String(path).startsWith("/_aibox/api/sessions?")),
    ).toHaveLength(1);
    expect(streamSessionDetail).toHaveBeenCalledTimes(1);

    view.rerender(<SessionPage api={api} search="" onLocationChange={onLocationChange} />);
    expect(await screen.findByRole("heading", { name: "Select a Session" })).toBeInTheDocument();
    await waitFor(() =>
      expect(
        get.mock.calls.filter(([path]) => String(path).startsWith("/_aibox/api/sessions?")),
      ).toHaveLength(2),
    );
  });
  it("keeps user messages separately and groups adjacent activity while preserving order", async () => {
    const streamSessionDetail = vi.fn(
      (_path: string, handlers: Parameters<ControlApi["streamSessionDetail"]>[1]) => {
        const meta: SessionDetailMeta = {
          id: firstSession.id,
          title: firstSession.title,
          start_ts: firstSession.start_ts,
          transcript_path: ".codex/session.jsonl",
          cwd: null,
          model_provider: null,
          cli_version: null,
        };
        const stats: SessionDetailStats = {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 1200,
          message_count: 2,
          tool_count: 0,
          entry_count: 4,
          malformed_count: 0,
          unsupported_count: 2,
          hidden_internal_count: 0,
          file_size: 128,
          snapshot: "128:1",
        };
        handlers.onMeta(meta);
        handlers.onMessage({
          entry_ids: ["message-1"],
          role: "user",
          timestamp: firstSession.start_ts,
          text: "Please inspect this.",
        });
        handlers.onEvidence({
          entry_id: "evidence-1",
          line: 2,
          timestamp: firstSession.start_ts,
          native_type: "response_item",
          role: null,
          content_types: [],
          status: "unsupported",
          preview: "first evidence",
        });
        handlers.onEvidence({
          entry_id: "evidence-2",
          line: 3,
          timestamp: firstSession.start_ts,
          native_type: "world_state",
          role: null,
          content_types: [],
          status: "filtered",
          preview: "second evidence",
        });
        handlers.onMessage({
          entry_ids: ["message-2"],
          role: "assistant",
          timestamp: firstSession.start_ts,
          text: "## Done",
        });
        handlers.onComplete(stats, ["encountered 2 unsupported Transcript Entry projection(s)"]);
      },
    );
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(screen.getByText(/· 1s · 2 messages · 0 tools/)).toBeInTheDocument();
    const userMessage = (await screen.findAllByRole("article")).find((article) =>
      article.textContent?.includes("Please inspect this."),
    );
    expect(userMessage).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Jump to message 1/ })).toHaveLength(2);
    const agentHeading = screen.getByRole("heading", { name: "Done" });
    expect(agentHeading).toBeInTheDocument();
    expect(agentHeading.closest("article")?.className).not.toContain("undefined");
    expect(screen.getByText("2 items · response_item, world_state")).toBeInTheDocument();
    expect(
      screen.getByText("Some transcript events could not be interpreted."),
    ).toBeInTheDocument();
  });
  it("collapses Transcript activity when refreshing Session detail", async () => {
    const streamSessionDetail = vi.fn(
      (_path: string, handlers: Parameters<ControlApi["streamSessionDetail"]>[1]) => {
        handlers.onEvidence({
          entry_id: "evidence-1",
          line: 2,
          timestamp: firstSession.start_ts,
          native_type: "response_item",
          role: null,
          content_types: [],
          status: "unsupported",
          preview: "evidence preview",
        });
        handlers.onComplete(
          {
            start_ts: firstSession.start_ts,
            last_event_ts: firstSession.start_ts,
            observed_duration_ms: 0,
            message_count: 0,
            tool_count: 0,
            entry_count: 1,
            malformed_count: 0,
            unsupported_count: 1,
            hidden_internal_count: 0,
            file_size: 128,
            snapshot: "128:1",
          },
          ["encountered 1 unsupported Transcript Entry projection(s)"],
        );
        return Promise.resolve();
      },
    );
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    const activitySummary = await screen.findByText("Transcript activity");
    const activityDisclosure = activitySummary.closest("details");
    expect(activityDisclosure).not.toBeNull();
    await user.click(activitySummary);
    expect(activityDisclosure).toHaveAttribute("open");
    await user.click(screen.getByRole("button", { name: "Refresh Session detail" }));
    await waitFor(() => expect(streamSessionDetail).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Transcript activity").closest("details")).not.toHaveAttribute("open");
  });
  it("offers Jump to latest when a long Conversation finishes loading at the beginning", async () => {
    const completion = deferred<void>();
    const streamSessionDetail = vi.fn(
      async (_path: string, handlers: Parameters<ControlApi["streamSessionDetail"]>[1]) => {
        handlers.onMessage({
          entry_ids: ["message-1"],
          role: "user",
          timestamp: firstSession.start_ts,
          text: "Please inspect the long Conversation.",
        });
        await completion.promise;
        handlers.onComplete(
          {
            start_ts: firstSession.start_ts,
            last_event_ts: firstSession.start_ts,
            observed_duration_ms: 0,
            message_count: 1,
            tool_count: 0,
            entry_count: 1,
            malformed_count: 0,
            unsupported_count: 0,
            hidden_internal_count: 0,
            file_size: 128,
            snapshot: "128:1",
          },
          [],
        );
      },
    );
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    const article = await screen.findByRole("article");
    const scrollContainer = article.parentElement?.parentElement as HTMLDivElement;
    Object.defineProperties(scrollContainer, {
      scrollHeight: { configurable: true, value: 1200 },
      scrollTop: { configurable: true, value: 0, writable: true },
      clientHeight: { configurable: true, value: 400 },
    });
    expect(screen.queryByRole("button", { name: "Jump to latest" })).not.toBeInTheDocument();
    act(() => completion.resolve());
    expect(await screen.findByRole("button", { name: "Jump to latest" })).toBeInTheDocument();
  });
  it("creates one navigation anchor for each user message", async () => {
    const streamSessionDetail = vi.fn(
      (_path: string, handlers: Parameters<ControlApi["streamSessionDetail"]>[1]) => {
        handlers.onMessage({
          entry_ids: ["user-1"],
          role: "user",
          timestamp: firstSession.start_ts,
          text: "First request",
        });
        handlers.onMessage({
          entry_ids: ["assistant-1"],
          role: "assistant",
          timestamp: firstSession.start_ts,
          text: "First answer",
        });
        handlers.onMessage({
          entry_ids: ["user-2"],
          role: "user",
          timestamp: firstSession.start_ts,
          text: "Second request",
        });
        handlers.onMessage({
          entry_ids: ["assistant-2"],
          role: "assistant",
          timestamp: firstSession.start_ts,
          text: "Second answer",
        });
        handlers.onComplete(
          {
            start_ts: firstSession.start_ts,
            last_event_ts: firstSession.start_ts,
            observed_duration_ms: 1200,
            message_count: 4,
            tool_count: 0,
            entry_count: 4,
            malformed_count: 0,
            unsupported_count: 0,
            hidden_internal_count: 0,
            file_size: 128,
            snapshot: "128:1",
          },
          [],
        );
      },
    );
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    const userArticles = screen
      .getAllByRole("article")
      .filter((article) =>
        ["First request", "Second request"].some((text) => within(article).queryByText(text)),
      );
    expect(userArticles).toHaveLength(2);
    expect(
      screen.getAllByRole("button", { name: /Jump to message 1: First request/ }),
    ).toHaveLength(2);
    expect(
      screen.getAllByRole("button", { name: /Jump to message 2: Second request/ }),
    ).toHaveLength(2);
    await user.click(screen.getAllByRole("button", { name: /Jump to message 2/ })[0]);
    expect(
      screen
        .getAllByRole("button", { name: /Jump to message 2/ })
        .every((button) => button.getAttribute("aria-current") === "location"),
    ).toBe(true);
  });
  it("reports an incomplete Transcript as a diagnostic", async () => {
    const streamSessionDetail = vi.fn().mockRejectedValue(new Error("truncated Transcript"));
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(await screen.findByText("Partial transcript")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Details/ }));
    expect(
      screen.getByText(
        "Transcript detail did not finish loading. Displayed content may be incomplete.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("No transcript diagnostics.")).not.toBeInTheDocument();
  });
  it("defaults to compact single-select Tenant and Agent menus", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    const tenantTrigger = await screen.findByRole("button", { name: "Tenant: default" });
    const agentTrigger = screen.getByRole("button", { name: "Coding Agent: Codex" });
    expect(tenantTrigger).toHaveTextContent("default");
    expect(tenantTrigger).not.toHaveTextContent("Tenant:");
    expect(agentTrigger).toHaveTextContent("Codex");
    expect(agentTrigger).not.toHaveTextContent("Coding Agent:");
    expect(agentTrigger.querySelector('[data-icon="codex"]')).toBeInTheDocument();
    await user.click(tenantTrigger);
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    expect(within(tenantMenu).getByRole("option", { name: "default" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(tenantMenu).getByRole("option", { name: "Host Tenant" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(
      within(tenantMenu).getByRole("button", { name: "Select multiple tenants" }),
    ).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(tenantTrigger).toHaveFocus();
    const session = await screen.findByRole("button", {
      name: "First prompt, Tenant default · Codex",
    });
    expect(session.querySelector('[data-icon="session-record"]')).toHaveClass(
      "lucide-messages-square",
    );
    expect(within(session).getByText("First prompt", { selector: "strong" })).toBeInTheDocument();
    const metadata = session.querySelector("small");
    expect(metadata).toHaveTextContent("default Codex");
    const sessionTime = within(metadata!).getByText("2026-08-17 17:00:00");
    expect(sessionTime.tagName).toBe("TIME");
    expect(sessionTime).toHaveAttribute("datetime", firstSession.start_ts);
    expect(metadata?.textContent).not.toContain("Codex 2026");
    expect(session).not.toHaveTextContent("Tenant");
    expect(session).not.toHaveTextContent(firstSession.display_id);
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toHaveAttribute("title", "Delete Session 111111111111 from Tenant default Codex");
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toHaveAttribute(
      "title",
      "Refresh Sessions",
    );
    expect(screen.getByRole("button", { name: "Select Sessions" })).toHaveTextContent("Select");
    await user.click(agentTrigger);
    const agentMenu = screen.getByRole("dialog", { name: "Coding Agent" });
    const codexOption = within(agentMenu).getByRole("option", { name: "Codex" });
    const claudeOption = within(agentMenu).getByRole("option", { name: "Claude" });
    expect(codexOption).toHaveAttribute("aria-selected", "true");
    expect(claudeOption).toHaveAttribute("aria-selected", "false");
    await user.click(claudeOption);
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toHaveTextContent(
      "Claude",
    );
  });
  it("reports a missing Managed Tenant in the Session selector", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/sessions?tenant=managed%3Amissing&agent=codex",
    );
    const { api } = fakeApi({ sessions: () => list([]) });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    const tenantTrigger = await screen.findByRole("button", { name: "Tenant: Not found" });
    expect(
      screen.getByText("No Sessions were found for the selected Tenants and Coding Agents."),
    ).toBeInTheDocument();
    await user.click(tenantTrigger);
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    for (const name of ["Host Tenant", "default", "work"]) {
      expect(within(tenantMenu).getByRole("option", { name })).toHaveAttribute(
        "aria-selected",
        "false",
      );
    }
  });
  it("keeps a complete long Session title in the two-line summary", async () => {
    const title =
      "A deliberately long Session title that remains available after its visual two-line clamp";
    const longSession = { ...firstSession, title };
    const { api } = fakeApi({ sessions: () => list([longSession]) });
    render(<SessionPage api={api} />);
    const session = await screen.findByRole("button", {
      name: `${title}, Tenant default · Codex`,
    });
    const titleElement = within(session).getByTitle(title);
    expect(titleElement.tagName).toBe("STRONG");
    expect(titleElement).toHaveTextContent(title);
  });
  it("stages multiple values, cancels drafts, and can return to one value", async () => {
    const { api, get } = fakeApi({
      sessions: (path) =>
        sessionQuery(path).get("agent") === "claude" ? list([secondSession]) : list([firstSession]),
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    const agentTrigger = screen.getByRole("button", { name: "Coding Agent: Codex" });
    await user.click(agentTrigger);
    let menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    const codexCheckbox = within(menu).getByRole("checkbox", { name: "Codex" });
    const claudeCheckbox = within(menu).getByRole("checkbox", { name: "Claude" });
    expect(codexCheckbox).toBeChecked();
    expect(codexCheckbox).toBeDisabled();
    expect(claudeCheckbox.closest("label")).toHaveAttribute("title", "Claude");
    await user.click(claudeCheckbox);
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeEnabled();
    expect(get.mock.calls.some(([path]) => String(path).includes("agent=claude"))).toBe(false);
    await user.keyboard("{Escape}");
    expect(agentTrigger).toHaveFocus();
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(document.body);
    expect(screen.queryByRole("dialog", { name: "Coding Agent" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(get).toHaveBeenCalledWith(
        expect.stringContaining("agent=claude"),
        expect.any(AbortSignal),
      ),
    );
    const multipleAgentTrigger = screen.getByRole("button", {
      name: "Coding Agent: 2 Coding Agents",
    });
    expect(multipleAgentTrigger).toHaveTextContent("2 Coding Agents");
    await user.click(multipleAgentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeDisabled();
    await user.click(within(menu).getByRole("checkbox", { name: "Codex" }));
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).not.toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeDisabled();
    expect(
      within(menu).getByRole("button", { name: "Choose one Coding Agent" }),
    ).toBeInTheDocument();
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Coding Agent: 2 Coding Agents" }));
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Choose one Coding Agent" }));
    await user.click(within(menu).getByRole("button", { name: "Back to multiple Coding Agents" }));
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    await user.click(within(menu).getByRole("button", { name: "Choose one Coding Agent" }));
    await user.click(within(menu).getByRole("option", { name: "Claude" }));
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toHaveTextContent(
      "Claude",
    );
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
  });
  it("aborts a stale Session list request when the Coding Agent changes", async () => {
    const codexList = deferred<SessionListData>();
    let codexCalls = 0;
    let codexSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (path, signal) => {
        if (path.includes("agent=codex")) {
          codexCalls += 1;
          if (codexCalls > 1) return list([firstSession]);
          codexSignal = signal;
          signal?.addEventListener("abort", () =>
            codexList.reject(new DOMException("Aborted", "AbortError")),
          );
          return codexList.promise;
        }
        return list([secondSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await waitFor(() => expect(codexSignal).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
    await user.click(screen.getByRole("option", { name: "Claude" }));
    expect(codexSignal?.aborted).toBe(true);
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
  });
  it("clears the manual refresh state when an Agent change replaces the request", async () => {
    const refresh = deferred<SessionListData>();
    let codexCalls = 0;
    let refreshSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (path, signal) => {
        if (path.includes("agent=claude")) return list([secondSession]);
        codexCalls += 1;
        if (codexCalls === 1) return list([firstSession]);
        if (codexCalls > 2) return list([firstSession]);
        refreshSignal = signal;
        signal?.addEventListener("abort", () =>
          refresh.reject(new DOMException("Aborted", "AbortError")),
        );
        return refresh.promise;
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Refresh Sessions" }));
    await waitFor(() => expect(refreshSignal).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
    await user.click(screen.getByRole("option", { name: "Claude" }));
    expect(refreshSignal?.aborted).toBe(true);
    await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" });
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeEnabled();
  });
  it("confirms one Session deletion, aborts its detail stream, and restores list focus", async () => {
    let rows = [firstSession, secondSession];
    const deletion = deferred<{
      deleted: number;
    }>();
    let detailSignal: AbortSignal | undefined;
    const post = vi.fn(() => deletion.promise);
    const streamSessionDetail = vi.fn((_path: string, _handlers: unknown, signal?: AbortSignal) => {
      detailSignal = signal;
      return new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      });
    });
    const { api } = fakeApi({ sessions: () => list(rows), post, streamSessionDetail });
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
      "This permanently deletes its Transcript from Tenant default Codex.",
    );
    expect(detailSignal?.aborted).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    expect(detailSignal?.aborted).toBe(true);
    expect(
      screen.getByRole("button", {
        name: "Deleting Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeDisabled();
    expect(post).toHaveBeenCalledWith("/_aibox/api/sessions/delete", {
      tenant: "managed:default",
      agent: "codex",
      ids: [firstSession.id],
      all: false,
      confirmation: "",
    });
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
    const post = vi.fn().mockResolvedValue({ deleted: 2 });
    const { api } = fakeApi({ sessions: () => list(rows), post });
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
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/sessions/delete", {
        tenant: "managed:default",
        agent: "codex",
        ids: [firstSession.id, secondSession.id],
        all: false,
        confirmation: "",
      }),
    );
    expect(
      await screen.findByRole("button", { name: "New prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Sessions" })).toBeInTheDocument();
  });
  it("reconciles surviving selections after a non-transactional batch failure", async () => {
    let rows = [firstSession, secondSession];
    const post = vi.fn().mockImplementation(() => {
      rows = [secondSession];
      return Promise.reject(new Error("second Transcript could not be deleted"));
    });
    const { api } = fakeApi({ sessions: () => list(rows), post });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));
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
    const { api, post } = fakeApi();
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
    expect(post).not.toHaveBeenCalled();
  });
  it("aggregates every selected Tenant and Coding Agent with stable source identities", async () => {
    const streamSessionDetail = vi.fn().mockResolvedValue(undefined);
    const { api, get } = fakeApi({
      sessions: (path) => {
        const query = sessionQuery(path);
        const tenantSelection = query.get("tenant") ?? "host";
        const tenant =
          tenantSelection === "host" ? "host" : tenantSelection.replace(/^managed:/, "");
        const agent = query.get("agent") ?? "codex";
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
    expect(get).toHaveBeenCalledWith(
      expect.stringContaining("tenant=managed%3Awork"),
      expect.any(AbortSignal),
    );
    expect(get).toHaveBeenCalledWith(
      expect.stringContaining("agent=claude"),
      expect.any(AbortSignal),
    );
    await user.click(newest);
    expect(streamSessionDetail).toHaveBeenCalledWith(
      expect.stringMatching(
        /tenant=managed%3Awork.*agent=claude|agent=claude.*tenant=managed%3Awork/,
      ),
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
      sessions: (path) => {
        if (sessionQuery(path).get("tenant") === "managed:work")
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
    const post = vi.fn(
      (
        _path: string,
        body: {
          tenant?: string;
        },
      ) => {
        if (body.tenant === "managed:default") {
          return defaultDeletion.promise.then((result) => {
            defaultRows = [];
            return result;
          });
        }
        return Promise.reject(new Error("work Transcript could not be deleted"));
      },
    );
    const { api } = fakeApi({
      sessions: (path) =>
        list(sessionQuery(path).get("tenant") === "managed:work" ? workRows : defaultRows),
      post,
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
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    expect(post).toHaveBeenCalledTimes(1);
    act(() => defaultDeletion.resolve({ deleted: 1 }));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
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
    expect(post.mock.calls[0][1]).toMatchObject({ tenant: "managed:default", agent: "codex" });
    expect(post.mock.calls[1][1]).toMatchObject({ tenant: "managed:work", agent: "codex" });
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
      screen.getByText("Choose a Session to inspect its conversation and Transcript evidence."),
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
