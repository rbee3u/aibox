import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { completedDetail } from "../test/fixtures";
import { RecordDetail } from "./RecordDetail";

function definitionValue(scope: HTMLElement, label: string): HTMLElement {
  const term = within(scope).getByText(label, { selector: "dt" });
  return term.parentElement!.querySelector("dd")!;
}

describe("Traffic Record Summary", () => {
  it("renders Claude cache fallback and all diagnostic groups", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          family: "claude_messages" as const,
          response_terminal: true,
          model: { requested: "claude-opus-5", effective: null },
          reasoning_effort: { requested: "high", effective: null },
          response_mode: { requested: "stream" as const, observed: "stream" as const },
          first_token_at_ns: "700000000",
          token_usage: {
            total_input_tokens: 415,
            base_input_tokens: 37,
            cached_input_tokens: 340,
            cache_write_tokens: 38,
            cache_write_5m_tokens: null,
            cache_write_1h_tokens: null,
            output_tokens: 13,
            reasoning_output_tokens: null,
          },
          errors: [{ kind: "overloaded_error", message: "Overloaded", at_ns: "800000000" }],
          warnings: [
            {
              kind: "cache_write_breakdown_inconsistent",
              message: "Cache write TTL details do not match the total.",
              at_ns: null,
            },
          ],
        },
        errors: [
          {
            phase: "response",
            kind: "client_disconnected",
            message: "Client disconnected.",
            at_ns: "900000000",
          },
        ],
        warnings: [
          {
            phase: "recording",
            kind: "event_index_failed",
            message: "Index warning.",
            at_ns: "1000000000",
          },
        ],
      },
    };
    render(
      <RecordDetail
        detail={detail}
        bodies={{ request: [], response: [] }}
        bodyStatus={{ request: "idle", response: "idle" }}
        tab="summary"
        onTabChange={vi.fn()}
        onDownload={vi.fn()}
        loadingBody={false}
      />,
    );

    const summary = screen.getByRole("tabpanel");
    const modelSummary = within(summary).getByRole("heading", {
      name: "Model",
    }).parentElement!;
    expect(within(summary).getByText("claude-opus-5")).toBeInTheDocument();
    expect(within(summary).getByText("high")).toBeInTheDocument();
    expect(within(modelSummary).getByText("Streaming")).toBeInTheDocument();
    const tokenUsage = within(summary).getByRole("region", { name: "Token usage" });
    const inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(Array.from(inputTokens.querySelectorAll("dt"), (term) => term.textContent)).toEqual([
      "Base input",
      "Cache hits & refreshes",
      "Cache writes",
    ]);
    for (const label of [
      "Total input",
      "Base input",
      "Cache hits & refreshes",
      "Cache writes",
      "Output",
    ]) {
      expect(within(tokenUsage).getByText(label)).toBeInTheDocument();
    }
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("38");
    expect(
      within(tokenUsage).getByRole("group", { name: "Total input tokens" }),
    ).toBeInTheDocument();
    expect(within(tokenUsage).getByRole("group", { name: "Output tokens" })).toBeInTheDocument();
    expect(within(summary).queryByText("Final")).not.toBeInTheDocument();
    expect(within(summary).queryByText(/Requested model/)).not.toBeInTheDocument();
    expect(
      within(summary).getByRole("region", { name: "API / Provider errors" }),
    ).toHaveTextContent("Overloaded");
    expect(within(summary).getByRole("region", { name: "Traffic errors" })).toHaveTextContent(
      "Client disconnected",
    );
    const warnings = within(summary).getByRole("region", { name: "Warnings" });
    expect(warnings).toHaveTextContent("Index warning");
    expect(warnings).toHaveTextContent("Cache write TTL details do not match the total");
  });

  it("nests the Claude TTL breakdown under a summed Cache writes metric", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          family: "claude_messages" as const,
          token_usage: {
            ...completedDetail.summary.protocol!.token_usage!,
            cache_write_tokens: null,
            cache_write_5m_tokens: 18,
            cache_write_1h_tokens: 20,
            reasoning_output_tokens: null,
          },
        },
      },
    };
    const { rerender } = render(
      <RecordDetail
        detail={detail}
        bodies={{ request: [], response: [] }}
        bodyStatus={{ request: "idle", response: "idle" }}
        tab="summary"
        onTabChange={vi.fn()}
        onDownload={vi.fn()}
        loadingBody={false}
      />,
    );

    const inputTokens = screen.getByRole("group", { name: "Input tokens" });
    expect(Array.from(inputTokens.querySelectorAll("dt"), (term) => term.textContent)).toEqual([
      "Base input",
      "Cache hits & refreshes",
      "Cache writes",
      "5m",
      "1h",
    ]);
    const cacheWrites = within(inputTokens).getByRole("group", {
      name: "Cache writes billing category",
    });
    expect(definitionValue(cacheWrites, "Cache writes")).toHaveTextContent("38");
    const breakdown = within(cacheWrites).getByRole("group", {
      name: "Cache write TTL breakdown",
    });
    expect(definitionValue(breakdown, "5m")).toHaveTextContent("18");
    expect(definitionValue(breakdown, "1h")).toHaveTextContent("20");

    rerender(
      <RecordDetail
        detail={{
          ...detail,
          summary: {
            ...detail.summary,
            protocol: {
              ...detail.summary.protocol,
              token_usage: {
                ...detail.summary.protocol.token_usage,
                cache_write_1h_tokens: null,
              },
            },
          },
        }}
        bodies={{ request: [], response: [] }}
        bodyStatus={{ request: "idle", response: "idle" }}
        tab="summary"
        onTabChange={vi.fn()}
        onDownload={vi.fn()}
        loadingBody={false}
      />,
    );
    const partialCacheWrites = screen.getByRole("group", {
      name: "Cache writes billing category",
    });
    expect(definitionValue(partialCacheWrites, "Cache writes")).toHaveTextContent("18");
    expect(
      definitionValue(
        within(partialCacheWrites).getByRole("group", { name: "Cache write TTL breakdown" }),
        "1h",
      ),
    ).toHaveTextContent("—");
  });

  it("prefers effective and observed values and orders Timing metrics", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          model: { requested: "requested-model", effective: "effective-model" },
          reasoning_effort: { requested: "high", effective: null },
          response_mode: { requested: "stream" as const, observed: "normal" as const },
        },
      },
    };
    render(
      <RecordDetail
        detail={detail}
        bodies={{ request: [], response: [] }}
        bodyStatus={{ request: "idle", response: "idle" }}
        tab="summary"
        onTabChange={vi.fn()}
        onDownload={vi.fn()}
        loadingBody={false}
      />,
    );

    const modelSummary = screen.getByRole("heading", { name: "Model" }).parentElement!;
    expect(within(modelSummary).getByTitle("Model effective-model")).toHaveTextContent(
      "effective-model·high",
    );
    expect(within(modelSummary).getByText("Non-streaming")).toBeInTheDocument();
    expect(screen.queryByText("requested-model")).not.toBeInTheDocument();
    const timingSection = screen.getByRole("heading", { name: "Timing" }).parentElement!;
    expect(Array.from(timingSection.querySelectorAll("dt"), (term) => term.textContent)).toEqual([
      "First token",
      "Duration",
      "Started",
    ]);
    expect(within(timingSection).getByRole("list", { name: "Timing stages" })).toHaveTextContent(
      "Response body",
    );
  });

  it("shows explicit Model states when protocol values are missing", () => {
    const props = {
      bodies: { request: [] as Uint8Array[], response: [] as Uint8Array[] },
      bodyStatus: { request: "idle" as const, response: "idle" as const },
      tab: "summary" as const,
      onTabChange: vi.fn(),
      onDownload: vi.fn(),
      loadingBody: false,
    };
    const terminal = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        coding_agent_session_id: null,
        protocol: null,
      },
    };
    const { rerender } = render(<RecordDetail detail={terminal} {...props} />);
    let modelSummary = screen.getByRole("heading", { name: "Model" }).parentElement!;
    expect(definitionValue(modelSummary, "Session ID")).toHaveTextContent("Not reported");
    expect(within(modelSummary).getByTitle("Model Not reported")).toHaveTextContent("Not reported");
    expect(within(modelSummary).queryByText("Reasoning effort")).not.toBeInTheDocument();
    expect(within(modelSummary).queryByText("Streaming")).not.toBeInTheDocument();

    const active = {
      ...completedDetail,
      state: "active" as const,
      result: null,
      live_total_ms: 500,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          model: { requested: null, effective: null },
          reasoning_effort: { requested: null, effective: null },
          response_mode: { requested: null, observed: null },
        },
      },
    };
    rerender(<RecordDetail detail={active} {...props} />);
    modelSummary = screen.getByRole("heading", { name: "Model" }).parentElement!;
    expect(within(modelSummary).getByTitle("Model Detecting…")).toHaveTextContent("Detecting…");
    expect(within(modelSummary).queryByText("Reasoning effort")).not.toBeInTheDocument();
    expect(within(modelSummary).queryByText("Streaming")).not.toBeInTheDocument();
  });

  it("derives usage state from the persisted protocol summary", () => {
    const props = {
      bodies: { request: [] as Uint8Array[], response: [] as Uint8Array[] },
      bodyStatus: { request: "idle" as const, response: "idle" as const },
      tab: "summary" as const,
      onTabChange: vi.fn(),
      onDownload: vi.fn(),
      loadingBody: false,
    };
    const oldRecord = {
      ...completedDetail,
      summary: { ...completedDetail.summary, protocol: null },
    };
    const { rerender } = render(<RecordDetail detail={oldRecord} {...props} />);
    expect(screen.getByText("Token usage is unavailable for this protocol.")).toBeInTheDocument();

    const terminalWithoutUsage = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: { ...completedDetail.summary.protocol!, token_usage: null },
      },
    };
    rerender(<RecordDetail detail={terminalWithoutUsage} {...props} />);
    expect(
      screen.getByText("The completed response did not report token usage."),
    ).toBeInTheDocument();

    const activeWithoutUsage = {
      ...terminalWithoutUsage,
      state: "active" as const,
      summary: {
        ...terminalWithoutUsage.summary,
        terminal: false,
        protocol: {
          ...terminalWithoutUsage.summary.protocol,
          response_terminal: false,
        },
      },
    };
    rerender(<RecordDetail detail={activeWithoutUsage} {...props} />);
    expect(
      screen.getByText("Waiting for the upstream API to report token usage."),
    ).toBeInTheDocument();
  });

  it("copies the Session ID and renders OpenAI token labels including zero", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    const props = {
      bodies: { request: [] as Uint8Array[], response: [] as Uint8Array[] },
      bodyStatus: { request: "idle" as const, response: "idle" as const },
      tab: "summary" as const,
      onTabChange: vi.fn(),
      onDownload: vi.fn(),
      loadingBody: false,
    };
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          token_usage: {
            ...completedDetail.summary.protocol!.token_usage!,
            output_tokens: 0,
          },
        },
      },
    };
    const { rerender } = render(<RecordDetail detail={detail} {...props} />);

    const tokenUsage = screen.getByRole("region", { name: "Token usage" });
    const inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(Array.from(inputTokens.querySelectorAll("dt"), (term) => term.textContent)).toEqual([
      "Input",
      "Cached input",
      "Cache writes",
    ]);
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("—");
    expect(screen.queryByText("Reasoning output")).not.toBeInTheDocument();
    const modelSummary = screen.getByRole("heading", { name: "Model" }).parentElement!;
    expect(definitionValue(modelSummary, "Output")).toHaveTextContent(/^0$/);
    const reasoning = within(tokenUsage).getByRole("group", {
      name: "Output includes 64 reasoning tokens",
    });
    expect(definitionValue(reasoning, "Reasoning")).toHaveTextContent("64");
    const copy = screen.getByRole("button", { name: "Copy Session ID" });
    await user.click(copy);
    expect(writeText).toHaveBeenCalledWith("629a8f94-d2cb-404c-9c10-a2a682478259");
    expect(screen.getByRole("button", { name: "Session ID copied" })).toBeInTheDocument();

    rerender(
      <RecordDetail
        detail={{
          ...detail,
          summary: { ...detail.summary, coding_agent_session_id: "different-session" },
        }}
        {...props}
      />,
    );
    expect(screen.getByRole("button", { name: "Copy Session ID" })).toBeInTheDocument();
  });

  it("preserves the token billing skeleton, missing markers, and explicit zero", () => {
    const props = {
      bodies: { request: [] as Uint8Array[], response: [] as Uint8Array[] },
      bodyStatus: { request: "idle" as const, response: "idle" as const },
      tab: "summary" as const,
      onTabChange: vi.fn(),
      onDownload: vi.fn(),
      loadingBody: false,
    };
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          token_usage: {
            total_input_tokens: null,
            base_input_tokens: 0,
            cached_input_tokens: null,
            cache_write_tokens: null,
            cache_write_5m_tokens: null,
            cache_write_1h_tokens: null,
            output_tokens: 5,
            reasoning_output_tokens: 2,
          },
        },
      },
    };
    const { rerender } = render(<RecordDetail detail={detail} {...props} />);

    let tokenUsage = screen.getByRole("region", { name: "Token usage" });
    let inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(definitionValue(inputTokens, "Input")).toHaveTextContent("0");
    expect(definitionValue(inputTokens, "Cached input")).toHaveTextContent("—");
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("—");
    expect(
      definitionValue(
        within(tokenUsage).getByRole("group", { name: "Total input tokens" }),
        "Total input",
      ),
    ).toHaveTextContent("—");
    expect(
      definitionValue(within(tokenUsage).getByRole("group", { name: "Output tokens" }), "Output"),
    ).toHaveTextContent("5");
    expect(
      within(tokenUsage).getByRole("group", { name: "Output includes 2 reasoning tokens" }),
    ).toBeInTheDocument();

    rerender(
      <RecordDetail
        detail={{
          ...detail,
          summary: {
            ...detail.summary,
            protocol: {
              ...detail.summary.protocol,
              token_usage: {
                ...detail.summary.protocol.token_usage,
                total_input_tokens: 0,
                base_input_tokens: null,
                output_tokens: null,
              },
            },
          },
        }}
        {...props}
      />,
    );
    tokenUsage = screen.getByRole("region", { name: "Token usage" });
    inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    for (const label of ["Input", "Cached input", "Cache writes"]) {
      expect(definitionValue(inputTokens, label)).toHaveTextContent("—");
    }
    expect(
      definitionValue(
        within(tokenUsage).getByRole("group", { name: "Total input tokens" }),
        "Total input",
      ),
    ).toHaveTextContent("0");
    expect(
      definitionValue(within(tokenUsage).getByRole("group", { name: "Output tokens" }), "Output"),
    ).toHaveTextContent("—");
    expect(within(tokenUsage).queryByText("Reasoning")).not.toBeInTheDocument();

    rerender(
      <RecordDetail
        detail={{
          ...detail,
          summary: {
            ...detail.summary,
            protocol: {
              ...detail.summary.protocol,
              token_usage: {
                total_input_tokens: null,
                base_input_tokens: null,
                cached_input_tokens: null,
                cache_write_tokens: null,
                cache_write_5m_tokens: null,
                cache_write_1h_tokens: null,
                output_tokens: null,
                reasoning_output_tokens: 2,
              },
            },
          },
        }}
        {...props}
      />,
    );
    expect(screen.getByText("The upstream API reported no token counters.")).toBeInTheDocument();
  });
});
