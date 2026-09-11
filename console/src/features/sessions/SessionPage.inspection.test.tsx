import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SessionDetailMeta, SessionDetailStats } from "@/api/sessions";
import type { SessionDetailHandlers } from "@/api/sessions";
import { deferred } from "@/test/deferred";
import { SessionPage, firstSession, list, fakeApi } from "@/features/sessions/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("keeps user messages separately and groups adjacent activity while preserving order", async () => {
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
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
    });
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(screen.getByText(/· 2 messages · 0 tools/)).toBeInTheDocument();
    expect(screen.queryByText(/· 1s ·/)).not.toBeInTheDocument();
    const userMessage = (await screen.findAllByRole("article")).find((article) =>
      article.textContent?.includes("Please inspect this."),
    );
    expect(userMessage).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Jump to message 1/ })).toHaveLength(2);
    const agentHeading = screen.getByRole("heading", { name: "Done" });
    expect(agentHeading).toBeInTheDocument();
    expect(agentHeading.closest("article")?.className).not.toContain("undefined");
    const activitySummary = screen.getByText("Transcript activity");
    const activityGroup = activitySummary.closest("details");
    const collapsedSummary = activitySummary.closest("summary");
    expect(collapsedSummary).toHaveTextContent("2 items");
    expect(collapsedSummary).not.toHaveTextContent("response_item");
    expect(collapsedSummary).not.toHaveTextContent("world_state");
    expect(activityGroup?.querySelector("summary svg")).toBeNull();
    expect(
      screen.queryByText("Some transcript events could not be interpreted."),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Transcript warning")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Transcript diagnostics")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Activity has diagnostics")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Details/ }));
    expect(screen.getByText("Unsupported")).toBeInTheDocument();
    expect(
      screen.queryByText("encountered 2 unsupported Transcript Entry projection(s)"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Observed span")).toBeInTheDocument();
    expect(screen.getByText("Session ID")).toBeInTheDocument();
    expect(screen.queryByText("Tenant")).not.toBeInTheDocument();
    expect(screen.queryByText("Coding Agent")).not.toBeInTheDocument();
    expect(screen.queryByText("Started")).not.toBeInTheDocument();
    expect(screen.getByTitle("First to last Transcript event")).toHaveTextContent("1s");
  });
  it("labels tool-bearing groups as tools and shows the command on the row", async () => {
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
      handlers.onMessage({
        entry_ids: ["message-1"],
        role: "user",
        timestamp: firstSession.start_ts,
        text: "Please inspect this.",
      });
      handlers.onTool({
        entry_ids: ["tool-1"],
        call_id: "call-1",
        timestamp: firstSession.start_ts,
        name: "exec",
        status: "started",
        summary: '{"cmd":"git status --porcelain"}',
      });
      handlers.onEvidence({
        entry_id: "evidence-1",
        line: 3,
        timestamp: firstSession.start_ts,
        native_type: "event_msg",
        role: null,
        content_types: [],
        status: "filtered",
        preview: "token",
      });
      handlers.onTool({
        entry_ids: ["tool-1-done"],
        call_id: "call-1",
        timestamp: firstSession.start_ts,
        name: "Tool result",
        status: "completed",
        summary: "Script completed",
      });
      handlers.onComplete(
        {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 1200,
          message_count: 1,
          tool_count: 1,
          entry_count: 3,
          malformed_count: 0,
          unsupported_count: 0,
          hidden_internal_count: 0,
          file_size: 128,
          snapshot: "128:1",
        },
        [],
      );
    });
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(screen.getByText("1 tool")).toBeInTheDocument();
    expect(screen.getByText("exec · 1 event")).toBeInTheDocument();
    expect(screen.queryByText("Transcript activity")).not.toBeInTheDocument();
    await user.click(screen.getByText("1 tool"));
    expect(screen.getByText(/exec · git status --porcelain/)).toBeInTheDocument();
    expect(screen.queryByText("Script completed")).not.toBeInTheDocument();
  });
  it("keeps Conversation alarms for malformed Transcript records", async () => {
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
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
        native_type: "malformed",
        role: null,
        content_types: [],
        status: "malformed",
        preview: "broken line",
      });
      handlers.onComplete(
        {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 0,
          message_count: 1,
          tool_count: 0,
          entry_count: 2,
          malformed_count: 1,
          unsupported_count: 0,
          hidden_internal_count: 0,
          file_size: 128,
          snapshot: "128:1",
        },
        ["line 2: malformed JSONL (invalid)"],
      );
    });
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(
      screen.getByText("Some transcript events could not be interpreted."),
    ).toBeInTheDocument();
    expect(screen.getByText("Transcript warning")).toBeInTheDocument();
    expect(screen.getByLabelText("Transcript diagnostics")).toBeInTheDocument();
    expect(screen.getByLabelText("Activity has diagnostics")).toBeInTheDocument();
    await user.click(
      within(screen.getByRole("navigation", { name: "Session views" })).getByRole("button", {
        name: /Details/,
      }),
    );
    expect(screen.getByText("Malformed")).toBeInTheDocument();
    expect(screen.getByText("line 2: malformed JSONL (invalid)")).toBeInTheDocument();
  });
  it("keeps leading evidence-only groups off Conversation and on Details", async () => {
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
      handlers.onEvidence({
        entry_id: "leading-1",
        line: 1,
        timestamp: firstSession.start_ts,
        native_type: "session_meta",
        role: null,
        content_types: [],
        status: "filtered",
        preview: "session meta",
      });
      handlers.onMessage({
        entry_ids: ["message-1"],
        role: "user",
        timestamp: firstSession.start_ts,
        text: "Please inspect this.",
      });
      handlers.onEvidence({
        entry_id: "between-1",
        line: 3,
        timestamp: firstSession.start_ts,
        native_type: "event_msg",
        role: null,
        content_types: [],
        status: "filtered",
        preview: "token",
      });
      handlers.onComplete(
        {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 0,
          message_count: 1,
          tool_count: 0,
          entry_count: 3,
          malformed_count: 0,
          unsupported_count: 1,
          hidden_internal_count: 0,
          file_size: 128,
          snapshot: "128:1",
        },
        [],
      );
    });
    const { api } = fakeApi({ sessions: () => list([firstSession]), streamSessionDetail });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(screen.getByRole("article")).toHaveTextContent("Please inspect this.");
    const activity = screen.getByText("Transcript activity");
    expect(activity.closest("summary")).toHaveTextContent("1 item");
    expect(screen.queryByText("session_meta")).not.toBeInTheDocument();
    await user.click(
      within(screen.getByRole("navigation", { name: "Session views" })).getByRole("button", {
        name: /Details/,
      }),
    );
    expect(screen.getByText("Transcript entries")).toBeInTheDocument();
    expect(screen.getByText("Unsupported")).toBeInTheDocument();
    expect(screen.getByText("Transcript entries").closest("div")).toHaveTextContent("3");
  });
  it("collapses Transcript activity when refreshing Session detail", async () => {
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
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
        preview: "evidence preview",
      });
      handlers.onComplete(
        {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 0,
          message_count: 1,
          tool_count: 0,
          entry_count: 2,
          malformed_count: 0,
          unsupported_count: 1,
          hidden_internal_count: 0,
          file_size: 128,
          snapshot: "128:1",
        },
        ["encountered 1 unsupported Transcript Entry projection(s)"],
      );
      return Promise.resolve();
    });
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
      async (_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
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
    const streamSessionDetail = vi.fn((_tenant, _agent, _id, handlers: SessionDetailHandlers) => {
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
    });
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
});
