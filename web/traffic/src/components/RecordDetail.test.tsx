import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { completedDetail } from "../test/fixtures";
import { RecordDetail } from "./RecordDetail";

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
    expect(within(summary).getByText("claude-opus-5")).toBeInTheDocument();
    expect(within(summary).getByText("high")).toBeInTheDocument();
    expect(
      within(summary).getByText("Requested model · Requested reasoning effort"),
    ).toBeInTheDocument();
    for (const label of [
      "Total input",
      "Base Input Tokens",
      "Cache Hits & Refreshes",
      "Cache writes",
      "Output Tokens",
    ]) {
      expect(within(summary).getByText(label)).toBeInTheDocument();
    }
    expect(within(summary).getByText("38")).toBeInTheDocument();
    expect(within(summary).getByText("Final")).toBeInTheDocument();
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

  it("resolves Model card values independently and orders Timing metrics", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          model: { requested: "requested-model", effective: "effective-model" },
          reasoning_effort: { requested: "high", effective: null },
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

    expect(screen.getByTitle("Model effective-model; Reasoning effort high")).toHaveTextContent(
      "effective-model·high",
    );
    expect(screen.getByText("Effective model · Requested reasoning effort")).toBeInTheDocument();
    const timingSection = screen.getByRole("heading", { name: "Timing" }).parentElement!;
    expect(Array.from(timingSection.querySelectorAll("dt"), (term) => term.textContent)).toEqual([
      "First token",
      "Duration",
      "Started",
    ]);
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
      summary: { ...completedDetail.summary, protocol: null },
    };
    const { rerender } = render(<RecordDetail detail={terminal} {...props} />);
    expect(screen.getByTitle("Model Not reported; Reasoning effort —")).toHaveTextContent(
      "Not reported·—",
    );

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
        },
      },
    };
    rerender(<RecordDetail detail={active} {...props} />);
    expect(screen.getByTitle("Model Detecting…; Reasoning effort —")).toHaveTextContent(
      "Detecting…·—",
    );
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
    expect(screen.getByText("Unsupported")).toBeInTheDocument();

    const terminalWithoutUsage = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: { ...completedDetail.summary.protocol!, token_usage: null },
      },
    };
    rerender(<RecordDetail detail={terminalWithoutUsage} {...props} />);
    expect(screen.getByText("Not reported")).toBeInTheDocument();

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
    expect(screen.getByText("Waiting")).toBeInTheDocument();
  });
});
