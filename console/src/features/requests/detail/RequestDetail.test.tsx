import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { LARGE_PRETTY_BYTES } from "@/features/requests/detail/bodyPresentation";
import {
  activeDetail,
  completedChatProtocol,
  completedDetail,
  withIncompleteRequestBody,
  withRequestEncoding,
} from "@/features/requests/testFixtures";
import type { RequestDetail as RequestDetailData, TokenUsage } from "@/api/requests";
import { RequestDetail } from "@/features/requests/detail/RequestDetail";
import styles from "@/features/requests/detail/RequestDetail.module.css";

type RequestDetailProps = ComponentProps<typeof RequestDetail>;
const zstdBytes = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);

function detailProps(
  detail: RequestDetailProps["detail"],
  overrides: Partial<Omit<RequestDetailProps, "detail">> = {},
): RequestDetailProps {
  return {
    detail,
    bodies: { request: [], response: [] },
    bodyStatus: { request: "idle", response: "idle" },
    decodedBodies: {
      request: { bytes: null, error: null },
      response: { bytes: null, error: null },
    },
    eventTimings: null,
    tab: "summary",
    onTabChange: vi.fn(),
    onDownload: vi.fn(),
    loadingBody: false,
    ...overrides,
  };
}

function renderDetail(
  detail: RequestDetailProps["detail"],
  overrides: Partial<Omit<RequestDetailProps, "detail">> = {},
) {
  return render(<RequestDetail {...detailProps(detail, overrides)} />);
}

function renderRequestBody(
  detail: RequestDetailProps["detail"],
  body: Uint8Array,
  overrides: Partial<Omit<RequestDetailProps, "detail" | "bodies" | "bodyStatus" | "tab">> = {},
) {
  return renderDetail(detail, {
    bodies: { request: [body], response: [] },
    bodyStatus: { request: "loaded", response: "idle" },
    tab: "request",
    ...overrides,
  });
}

function definitionValue(scope: HTMLElement, label: string): HTMLElement {
  const term = within(scope).getByText(label, { selector: "dt" });
  const definition = term.parentElement?.querySelector<HTMLElement>("dd");
  if (!definition) throw new Error(`Missing definition for ${label}`);
  return definition;
}

function terms(scope: HTMLElement): string[] {
  return within(scope)
    .getAllByRole("term")
    .map((term) => term.textContent ?? "");
}

function withTokenUsage(detail: RequestDetailData, tokenUsage: TokenUsage): RequestDetailData {
  return {
    ...detail,
    summary: {
      ...detail.summary,
      protocol: { ...detail.summary.protocol!, token_usage: tokenUsage },
    },
  };
}

describe("RequestDetail", () => {
  it("renders Claude cache fallback and each diagnostic group", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol,
          family: "claude_messages" as const,
          model: { requested: "claude-opus-5", effective: null },
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
        },
      },
      diagnostics: {
        provider: [
          {
            level: "error" as const,
            source: "provider" as const,
            kind: "overloaded_error",
            message: "Overloaded",
            phase: "model_api",
            at_ns: "800000000",
          },
        ],
        request: [
          {
            level: "error" as const,
            source: "request" as const,
            kind: "client_disconnected",
            message: "Client disconnected.",
            phase: "response",
            at_ns: "900000000",
          },
        ],
        http: [
          {
            level: "error" as const,
            source: "http" as const,
            kind: "http_503",
            message: "Upstream returned HTTP 503",
            phase: "response",
            at_ns: "500000000",
          },
        ],
        warnings: [
          {
            level: "warning" as const,
            source: "diagnostic" as const,
            kind: "event_index_failed",
            message: "Index warning.",
            phase: "recording",
            at_ns: "1000000000",
          },
          {
            level: "warning" as const,
            source: "diagnostic" as const,
            kind: "cache_write_breakdown_inconsistent",
            message: "Cache write TTL details do not match the total.",
            phase: "model_api",
            at_ns: null,
          },
        ],
      },
    };
    renderDetail(detail);

    const summary = screen.getByRole("tabpanel");
    const modelSummary = within(summary).getByRole("region", { name: "Model" });
    expect(within(summary).getByText("claude-opus-5")).toBeInTheDocument();
    expect(within(summary).getByText("high")).toBeInTheDocument();
    expect(within(modelSummary).getByText("Streaming")).toBeInTheDocument();
    const tokenUsage = within(summary).getByRole("region", { name: "Token usage" });
    const inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(terms(inputTokens)).toEqual(["Base input", "Cache hits & refreshes", "Cache writes"]);
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("38");
    expect(
      within(tokenUsage).getByRole("group", { name: "Total input tokens" }),
    ).toBeInTheDocument();
    expect(within(tokenUsage).getByRole("group", { name: "Output tokens" })).toBeInTheDocument();
    expect(within(summary).queryByText("Final")).not.toBeInTheDocument();
    expect(within(summary).queryByText(/Requested model/)).not.toBeInTheDocument();
    expect(within(summary).getByRole("region", { name: "Model API" })).toHaveTextContent(
      "Overloaded",
    );
    expect(within(summary).getByRole("region", { name: "Proxy / transport" })).toHaveTextContent(
      "Client disconnected",
    );
    expect(within(summary).getByRole("region", { name: "HTTP response" })).toHaveTextContent(
      "Upstream returned HTTP 503",
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
          ...completedDetail.summary.protocol,
          family: "claude_messages" as const,
          token_usage: {
            ...completedDetail.summary.protocol.token_usage,
            cache_write_tokens: null,
            cache_write_5m_tokens: 18,
            cache_write_1h_tokens: 20,
            reasoning_output_tokens: null,
          },
        },
      },
    };
    const { rerender } = renderDetail(detail);

    const inputTokens = screen.getByRole("group", { name: "Input tokens" });
    expect(terms(inputTokens)).toEqual([
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
      <RequestDetail
        {...detailProps({
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
        })}
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
          ...completedDetail.summary.protocol,
          model: { requested: "requested-model", effective: "effective-model" },
          reasoning_effort: { requested: "high", effective: null },
          response_mode: { requested: "stream" as const, observed: "normal" as const },
        },
      },
    };
    renderDetail(detail);

    const modelSummary = screen.getByRole("region", { name: "Model" });
    expect(within(modelSummary).getByTitle("Model effective-model")).toHaveTextContent(
      "effective-model high",
    );
    expect(within(modelSummary).getByText("high")).toHaveClass(styles.modelEffort);
    expect(within(modelSummary).getByText("Non-streaming")).toBeInTheDocument();
    expect(screen.queryByText("requested-model")).not.toBeInTheDocument();
    const timingSection = screen.getByRole("region", { name: "Timing" });
    expect(terms(timingSection)).toEqual(["First token", "Duration", "Ended"]);
    expect(definitionValue(timingSection, "Ended")).toHaveTextContent("2026-08-06 12:00:01");
    expect(within(timingSection).getByRole("list", { name: "Timing stages" })).toHaveTextContent(
      "Response body",
    );
  });

  it("moves focus through detail tabs with the keyboard", () => {
    const onTabChange = vi.fn();
    renderDetail(completedDetail, { onTabChange });

    const tabs = {
      summary: screen.getByRole("tab", { name: "Summary" }),
      request: screen.getByRole("tab", { name: "Request" }),
      response: screen.getByRole("tab", { name: "Response" }),
    };
    const cases = [
      ["summary", "ArrowRight", "request"],
      ["summary", "ArrowLeft", "response"],
      ["response", "ArrowRight", "summary"],
      ["request", "Home", "summary"],
      ["request", "End", "response"],
    ] as const;

    for (const [from, key, to] of cases) {
      tabs[from].focus();
      fireEvent.keyDown(tabs[from], { key });
      expect(tabs[to]).toHaveFocus();
      expect(onTabChange).toHaveBeenLastCalledWith(to);
    }
  });

  it("shows no End Time while a Request is active", () => {
    renderDetail(activeDetail);

    const timingSection = screen.getByRole("region", { name: "Timing" });
    expect(definitionValue(timingSection, "Ended")).toHaveTextContent("—");
  });

  it("shows explicit Model states when protocol values are missing", () => {
    const terminal = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        coding_agent_session_id: null,
        protocol: null,
      },
    };
    const { rerender } = renderDetail(terminal);
    let modelSummary = screen.getByRole("region", { name: "Model" });
    expect(definitionValue(modelSummary, "Coding Agent Session ID")).toHaveTextContent(
      "Not reported",
    );
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
          ...completedDetail.summary.protocol,
          model: { requested: null, effective: null },
          reasoning_effort: { requested: null, effective: null },
          response_mode: { requested: null, observed: null },
        },
      },
    };
    rerender(<RequestDetail {...detailProps(active)} />);
    modelSummary = screen.getByRole("region", { name: "Model" });
    expect(within(modelSummary).getByTitle("Model Detecting…")).toHaveTextContent("Detecting…");
    expect(within(modelSummary).queryByText("Reasoning effort")).not.toBeInTheDocument();
    expect(within(modelSummary).queryByText("Streaming")).not.toBeInTheDocument();
  });

  it("derives usage state from the persisted protocol summary", () => {
    const oldRequest = {
      ...completedDetail,
      summary: { ...completedDetail.summary, protocol: null },
    };
    const { rerender } = renderDetail(oldRequest);
    expect(screen.getByText("Token usage is unavailable for this protocol.")).toBeInTheDocument();

    const terminalWithoutUsage = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: { ...completedDetail.summary.protocol, token_usage: null },
      },
    };
    rerender(<RequestDetail {...detailProps(terminalWithoutUsage)} />);
    expect(
      screen.getByText("The completed response did not report token usage."),
    ).toBeInTheDocument();

    const failedWithoutUsage = {
      ...terminalWithoutUsage,
      summary: {
        ...terminalWithoutUsage.summary,
        outcome: "upstream_error" as const,
        protocol: {
          ...terminalWithoutUsage.summary.protocol,
          response_terminal: false,
        },
      },
    };
    rerender(<RequestDetail {...detailProps(failedWithoutUsage)} />);
    expect(
      screen.getByText("Token usage was not reported before this request ended."),
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
    rerender(<RequestDetail {...detailProps(activeWithoutUsage)} />);
    expect(
      screen.getByText("Waiting for the upstream API to report token usage."),
    ).toBeInTheDocument();
  });

  it("copies the Coding Agent Session ID and renders OpenAI token labels including zero", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    const detail = withTokenUsage(completedDetail, {
      ...completedDetail.summary.protocol.token_usage,
      output_tokens: 0,
    });
    const { rerender } = renderDetail(detail);

    const tokenUsage = screen.getByRole("region", { name: "Token usage" });
    const inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(terms(inputTokens)).toEqual(["Input", "Cached input", "Cache writes"]);
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("—");
    expect(screen.queryByText("Reasoning output")).not.toBeInTheDocument();
    const modelSummary = screen.getByRole("region", { name: "Model" });
    expect(definitionValue(modelSummary, "Output")).toHaveTextContent(/^0$/);
    const reasoning = within(tokenUsage).getByRole("group", {
      name: "Output includes 64 reasoning tokens",
    });
    expect(definitionValue(reasoning, "Reasoning")).toHaveTextContent("64");
    const copy = screen.getByRole("button", { name: "Copy Coding Agent Session ID" });
    await user.click(copy);
    expect(writeText).toHaveBeenCalledWith("629a8f94-d2cb-404c-9c10-a2a682478259");
    expect(
      screen.getByRole("button", { name: "Coding Agent Session ID copied" }),
    ).toBeInTheDocument();

    rerender(
      <RequestDetail
        {...detailProps({
          ...detail,
          summary: { ...detail.summary, coding_agent_session_id: "different-session" },
        })}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Copy Coding Agent Session ID" }),
    ).toBeInTheDocument();
  });

  it("renders Chat Completions with the existing OpenAI token hierarchy", () => {
    renderDetail({
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: completedChatProtocol,
      },
    });

    const modelSummary = screen.getByRole("region", { name: "Model" });
    expect(within(modelSummary).getByTitle("Model gpt-chat-2026-08-01")).toBeInTheDocument();
    const tokenUsage = screen.getByRole("region", { name: "Token usage" });
    const inputTokens = within(tokenUsage).getByRole("group", { name: "Input tokens" });
    expect(terms(inputTokens)).toEqual(["Input", "Cached input", "Cache writes"]);
    expect(definitionValue(inputTokens, "Input")).toHaveTextContent("100");
    expect(definitionValue(inputTokens, "Cached input")).toHaveTextContent("40");
    expect(definitionValue(inputTokens, "Cache writes")).toHaveTextContent("10");
  });

  it("preserves the token billing skeleton, missing markers, and explicit zero", () => {
    const detail = withTokenUsage(completedDetail, {
      total_input_tokens: null,
      base_input_tokens: 0,
      cached_input_tokens: null,
      cache_write_tokens: null,
      cache_write_5m_tokens: null,
      cache_write_1h_tokens: null,
      output_tokens: 5,
      reasoning_output_tokens: 2,
    });
    const { rerender } = renderDetail(detail);

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
      <RequestDetail
        {...detailProps(
          withTokenUsage(detail, {
            ...detail.summary.protocol!.token_usage!,
            total_input_tokens: 0,
            base_input_tokens: null,
            output_tokens: null,
          }),
        )}
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
      <RequestDetail
        {...detailProps(
          withTokenUsage(detail, {
            total_input_tokens: null,
            base_input_tokens: null,
            cached_input_tokens: null,
            cache_write_tokens: null,
            cache_write_5m_tokens: null,
            cache_write_1h_tokens: null,
            output_tokens: null,
            reasoning_output_tokens: 2,
          }),
        )}
      />,
    );
    expect(screen.getByText("The upstream API reported no token counters.")).toBeInTheDocument();
  });

  it("defaults JSON Bodies to Pretty, folds nested values, and copies losslessly", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    const source = '{"nested":{"big":900719925474099312345},"text":"' + `${"界".repeat(201)}"}`;
    const encoded = new TextEncoder().encode(source);
    renderRequestBody({ ...completedDetail, request_body_bytes: encoded.length }, encoded);

    expect(screen.getByRole("button", { name: "Pretty" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Expand nested" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.queryByText("900719925474099312345")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand nested" }));
    expect(screen.getByText("900719925474099312345")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy string value" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Copy object value" })).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Copy decoded Body Source" }));
    expect(writeText).toHaveBeenLastCalledWith(source);
    await user.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.getByText(source)).toBeInTheDocument();
    expect(screen.queryByText(/No Pretty renderer/)).not.toBeInTheDocument();
  });

  it("navigates visible JSON nodes with the ARIA tree keyboard model", () => {
    const source = '{"nested":{"answer":42},"tail":true}';
    renderRequestBody(
      { ...completedDetail, request_body_bytes: source.length },
      new TextEncoder().encode(source),
    );

    const initialItems = screen.getAllByRole("treeitem");
    const root = initialItems[0];
    const nested = initialItems[1];
    expect(root).toHaveAttribute("tabindex", "0");
    expect(nested).toHaveAttribute("aria-expanded", "false");

    root.focus();
    fireEvent.keyDown(root, { key: "ArrowDown" });
    expect(nested).toHaveFocus();
    fireEvent.keyDown(nested, { key: "ArrowRight" });
    expect(nested).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(nested, { key: "ArrowRight" });
    const expandedItems = screen.getAllByRole("treeitem");
    expect(expandedItems[2]).toHaveFocus();
    fireEvent.keyDown(expandedItems[2], { key: "ArrowLeft" });
    expect(nested).toHaveFocus();
    fireEvent.keyDown(nested, { key: "End" });
    expect(screen.getAllByRole("treeitem").at(-1)).toHaveFocus();
  });

  it("keeps the real fallback reason when declared JSON cannot be rendered", () => {
    const source = '{"broken":';
    renderRequestBody(
      { ...completedDetail, request_body_bytes: source.length },
      new TextEncoder().encode(source),
    );

    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Pretty unavailable:");
  });

  it("derives the gzip wait state from Body completeness", () => {
    const source = new Uint8Array([0x1f, 0x8b]);
    renderRequestBody(
      {
        ...withIncompleteRequestBody(withRequestEncoding(activeDetail, "gzip")),
        request_body_bytes: source.length,
      },
      source,
      {
        decodedBodies: {
          request: { bytes: null, error: null },
          response: { bytes: null, error: null },
        },
      },
    );

    expect(screen.getByRole("button", { name: "Pretty" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for the complete gzip Body before decoding",
    );
  });

  it("shows unsupported encoding instead of waiting for an incomplete JSON Body", () => {
    const source = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);
    renderRequestBody(
      {
        ...withIncompleteRequestBody(withRequestEncoding(activeDetail, "br")),
        request_body_bytes: source.length,
      },
      source,
    );

    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Unsupported Content-Encoding: br");
    expect(screen.queryByText(/Waiting for the complete JSON Body/)).not.toBeInTheDocument();
  });

  it("derives the zstd wait state from Body completeness", () => {
    renderRequestBody(
      {
        ...withIncompleteRequestBody(withRequestEncoding(activeDetail, "zstd")),
        request_body_bytes: zstdBytes.length,
      },
      zstdBytes,
      {
        decodedBodies: {
          request: { bytes: null, error: null },
          response: { bytes: null, error: null },
        },
      },
    );

    expect(screen.getByRole("button", { name: "Pretty" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for the complete zstd Body before decoding",
    );
  });

  it("does not offer Pretty rendering for a large non-UTF-8 Body", () => {
    renderRequestBody(
      { ...completedDetail, request_body_bytes: LARGE_PRETTY_BYTES + 1 },
      new Uint8Array([0xff]),
    );

    expect(screen.getByRole("status")).toHaveTextContent("not valid UTF-8");
    expect(screen.queryByRole("button", { name: "Render Pretty" })).not.toBeInTheDocument();
  });

  it("shows decoded zstd bytes as hex when the decoded Source is not UTF-8", () => {
    const decoded = new Uint8Array([0xff, 0x00]);
    renderRequestBody(
      { ...withRequestEncoding(completedDetail, "zstd"), request_body_bytes: zstdBytes.length },
      zstdBytes,
      {
        decodedBodies: {
          request: { bytes: decoded, error: null },
          response: { bytes: null, error: null },
        },
      },
    );

    expect(screen.getByText("hex: ff 00")).toBeInTheDocument();
    expect(screen.queryByText(/28 b5 2f fd/)).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("showing decoded bytes as hex");
  });

  it("shows SSE Event type, completion time, partial timing warning, and Event data copy", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    const source =
      'event: transport.delta\ndata: {"type":"answer.delta","value":900719925474099312345}\n\n' +
      "data: [DONE]\n\n";
    renderDetail(
      { ...completedDetail, response_body_bytes: source.length },
      {
        bodies: { request: [], response: [new TextEncoder().encode(source)] },
        bodyStatus: { request: "idle", response: "loaded" },
        eventTimings: {
          state: "partial",
          events: [{ sequence: 0, completed_at_ns: "1250500000" }],
          next_sequence: 1,
          warning: "SSE Event timing index is incomplete.",
        },
        tab: "response",
      },
    );

    expect(screen.getByText("SSE Event timing index is incomplete.")).toBeInTheDocument();
    const eventList = screen.getByRole("list", { name: "SSE Events" });
    expect(within(eventList).getAllByRole("listitem")).toHaveLength(2);
    const first = screen.getByRole("button", { name: /answer.delta/ });
    expect(first).toHaveTextContent("transport.delta");
    expect(screen.getByText("+1.251 s")).toHaveAttribute("title", "2026-08-06 12:00:01.251");
    expect(screen.getByText("Time unavailable")).toBeInTheDocument();
    await user.click(first);
    expect(screen.getByText("900719925474099312345")).toBeInTheDocument();
    const copyButtons = screen.getAllByRole("button", { name: "Copy SSE Event data" });
    await user.click(copyButtons[0]);
    expect(writeText).toHaveBeenLastCalledWith(
      '{\n  "type": "answer.delta",\n  "value": 900719925474099312345\n}',
    );
    await user.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.queryByText(/No Pretty renderer/)).not.toBeInTheDocument();
  });

  it("keeps Chat content and tool-call deltas as inspectable raw Events", async () => {
    const user = userEvent.setup();
    const payload = JSON.stringify({
      object: "chat.completion.chunk",
      choices: [
        {
          delta: {
            content: "Hello",
            tool_calls: [{ function: { arguments: '{"city":"San' } }],
          },
          finish_reason: null,
        },
      ],
    });
    const source = `data: ${payload}\n\ndata: [DONE]\n\n`;
    renderDetail(
      { ...completedDetail, response_body_bytes: source.length },
      {
        bodies: { request: [], response: [new TextEncoder().encode(source)] },
        bodyStatus: { request: "idle", response: "loaded" },
        tab: "response",
      },
    );

    const chunk = screen.getByRole("button", { name: /chat\.completion\.chunk/ });
    await user.click(chunk);
    await user.click(screen.getByRole("button", { name: "Expand choices" }));
    await user.click(screen.getByRole("button", { name: "Expand 0" }));
    await user.click(screen.getByRole("button", { name: "Expand delta" }));
    expect(screen.getByText(/Hello/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand tool_calls" }));
    await user.click(screen.getByRole("button", { name: "Expand 0" }));
    await user.click(screen.getByRole("button", { name: "Expand function" }));
    expect(screen.getByText(/city.*San/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /message/ })).toBeInTheDocument();
  });
});
