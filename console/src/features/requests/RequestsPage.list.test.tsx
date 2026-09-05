import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import {
  activeSummary,
  completedSummary,
  completedSummaryFor,
  requestList,
  requestListFor,
  requestsApiFake,
} from "@/features/requests/testFixtures";
import {
  RequestsHarness,
  advanceTimers,
  flushEffects,
  renderApp,
} from "@/features/requests/testHarness";
import { deferred } from "@/test/deferred";

const fileSystem = (
  globalThis as typeof globalThis & {
    process: {
      getBuiltinModule(name: "fs"): {
        readFileSync(path: string, encoding: "utf8"): string;
      };
    };
  }
).process.getBuiltinModule("fs");
const requestListCss = fileSystem.readFileSync(
  "src/features/requests/catalog/RequestList.module.css",
  "utf8",
);

describe("Requests page list", () => {
  it("keeps long targets shrinkable while reserving the status column", () => {
    expect(requestListCss).toMatch(
      /\.rowButton\s*\{[\s\S]*?grid-template-columns:\s*20px 45px minmax\(0, 1fr\) max-content;[\s\S]*?min-width:\s*0;/s,
    );
    expect(requestListCss).toMatch(
      /\.target\s*\{[\s\S]*?min-width:\s*0;[\s\S]*?overflow:\s*hidden;[\s\S]*?text-overflow:\s*ellipsis;[\s\S]*?white-space:\s*nowrap;/s,
    );
    expect(requestListCss).toMatch(
      /\.status\s*\{[\s\S]*?min-width:\s*max-content;[\s\S]*?padding-left:\s*var\(--space-2\);[\s\S]*?white-space:\s*nowrap;/s,
    );
    expect(requestListCss).toMatch(
      /\.catalogIssueSlot\s*\{[\s\S]*?min-width:\s*0;[\s\S]*?max-width:\s*100%;/s,
    );
  });

  it("preserves target prefixes, full URL titles, and status content for long URLs", async () => {
    const longHost = `gateway.${"regional-".repeat(7)}example.test`;
    const longPath = `/v1/${"organizations/".repeat(7)}responses`;
    const longQuery = `?stream=true&session=${"session-token-".repeat(8)}&include=usage`;
    const cases = [
      {
        id: "long-host-request",
        url: `https://${longHost}/v1/responses?stream=true`,
        label: `${longHost}/v1/responses`,
      },
      {
        id: "long-path-request",
        url: `https://api.example.test${longPath}`,
        label: `api.example.test${longPath}`,
      },
      {
        id: "long-query-request",
        url: `https://api.example.test/v1/responses${longQuery}`,
        label: "api.example.test/v1/responses",
      },
    ];
    const longRequests = cases.map(({ id, url }) => ({
      ...activeSummary,
      id,
      method: "POST",
      incoming_uri: `/${url}`,
      upstream_url: url,
      status: 200,
    }));

    renderApp({ listRequests: vi.fn().mockResolvedValue(requestListFor(longRequests)) });
    const panel = await screen.findByRole("complementary", { name: "Request list" });

    for (const { url, label } of cases) {
      const row = within(panel).getByRole("button", { name: `POST ${label}` });
      const target = within(row).getByTitle(url);
      expect(target).toHaveTextContent(label);
      expect(target.textContent).not.toContain("stream=true");
      expect(within(row).getByText("200")).toBeInTheDocument();
      expect(within(row).getByText("Streaming")).toBeInTheDocument();
      expect(row).toHaveAccessibleDescription(
        expect.stringContaining("Duration 500ms; Started 2026-08-06 12:01:00"),
      );
    }
  });

  it("renders concise Request summaries in the Console module", async () => {
    renderApp();

    const requestListPanel = await screen.findByRole("complementary", {
      name: "Request list",
    });
    expect(within(requestListPanel).queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
    expect(
      within(requestListPanel).getByRole("button", { name: "Refresh Requests" }),
    ).toBeEnabled();
    expect(within(requestListPanel).getByRole("button", { name: "Select Requests" })).toBeEnabled();
    expect(within(requestListPanel).queryByRole("button", { name: "Delete all" })).toBeNull();
    const completedRow = within(requestListPanel).getByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    const completedModel = within(completedRow).getByTitle(
      "Model gpt-5.6-sol; Reasoning effort high",
    );
    const completedTiming = within(completedRow).getByTitle("First token 900ms; Duration 1s");
    const completedEnded = within(completedRow).getByTitle("Ended 2026-08-06 12:00:01");
    const requestIcon = completedRow.querySelector('[data-icon="request-row"]');
    expect(requestIcon).toHaveClass("lucide-arrow-left-right");
    expect(requestIcon).toHaveAttribute("aria-hidden", "true");
    expect(completedModel).toHaveTextContent("gpt-5.6-sol high");
    expect(completedTiming).toHaveTextContent("900ms / 1s");
    expect(completedTiming.parentElement).toContainElement(completedEnded);
    const completedTarget = within(completedRow).getByTitle(
      "https://api.example.test/v1/responses?stream=true",
    );
    expect(completedTarget).toHaveTextContent("api.example.test/v1/responses");
    expect(within(completedRow).queryByText(/stream=true/)).not.toBeInTheDocument();
    expect(completedEnded).toHaveTextContent("2026-08-06 12:00:01");
    expect(within(completedRow).getByText("200")).toBeInTheDocument();
    expect(completedRow).toHaveAccessibleDescription(
      "Model gpt-5.6-sol; Reasoning effort high; First token 900ms; Duration 1s; Ended 2026-08-06 12:00:01",
    );
    const activeRow = within(requestListPanel).getByRole("button", {
      name: "GET stream.example.test/events",
    });
    const activeStarted = within(activeRow).getByTitle("Started 2026-08-06 12:01:00");
    expect(activeStarted).toHaveTextContent("2026-08-06 12:01:00");
    expect(activeStarted).toHaveAttribute("datetime", activeSummary.started_at);
    expect(activeStarted.tagName).toBe("TIME");
    expect(activeRow).toHaveAccessibleDescription(
      "Model gpt-5.6-sol; Reasoning effort high; First token —; Duration 500ms; Started 2026-08-06 12:01:00",
    );
    expect(within(requestListPanel).getByTitle("First token —; Duration 500ms")).toHaveTextContent(
      "— / 500ms",
    );
  });

  it("prefers effective list metadata and preserves placeholders", async () => {
    const effective = {
      ...completedSummary,
      id: "effective-request",
      incoming_uri: "/https://api.example.test/effective",
      upstream_url: "https://api.example.test/effective",
      protocol: {
        ...completedSummary.protocol,
        model: { requested: "requested-model", effective: "effective-model" },
        reasoning_effort: { requested: "low", effective: "xhigh" },
      },
    };
    const requestedFallback = {
      ...completedSummary,
      id: "requested-request",
      incoming_uri: "/https://api.example.test/requested",
      upstream_url: "https://api.example.test/requested",
      total_ms: null,
      protocol: {
        ...completedSummary.protocol,
        model: { requested: null, effective: null },
        reasoning_effort: { requested: "medium", effective: null },
        first_token_at_ns: null,
      },
    };
    const legacy = {
      ...completedSummary,
      id: "legacy-request",
      incoming_uri: "/https://api.example.test/legacy",
      upstream_url: "https://api.example.test/legacy",
      protocol: null,
    };
    const missingEffort = {
      ...completedSummary,
      id: "missing-effort-request",
      incoming_uri: "/https://api.example.test/missing-effort",
      upstream_url: "https://api.example.test/missing-effort",
      protocol: {
        ...completedSummary.protocol,
        reasoning_effort: { requested: null, effective: null },
      },
    };
    const longTiming = {
      ...completedSummary,
      id: "long-timing-request",
      incoming_uri: "/https://api.example.test/long-timing",
      upstream_url: "https://api.example.test/long-timing",
      total_ms: 1_735_000,
      protocol: {
        ...completedSummary.protocol,
        first_token_at_ns: "1054000000000",
      },
    };
    renderApp({
      listRequests: vi
        .fn()
        .mockResolvedValue(
          requestListFor([effective, requestedFallback, legacy, missingEffort, longTiming]),
        ),
    });

    const effectiveRow = await screen.findByRole("button", {
      name: "POST api.example.test/effective",
    });
    expect(
      within(effectiveRow).getByTitle("Model effective-model; Reasoning effort xhigh"),
    ).toHaveTextContent("effective-model xhigh");
    expect(within(effectiveRow).getByTitle(/^Ended /)).toHaveAttribute(
      "datetime",
      effective.ended_at,
    );
    expect(within(effectiveRow).getByTitle(/^Ended /).tagName).toBe("TIME");

    const requestedRow = screen.getByRole("button", {
      name: "POST api.example.test/requested",
    });
    expect(within(requestedRow).getByTitle("Model —; Reasoning effort medium")).toHaveTextContent(
      "— medium",
    );
    expect(within(requestedRow).getByTitle("First token —; Duration —")).toHaveTextContent("— / —");

    const legacyRow = screen.getByRole("button", { name: "POST api.example.test/legacy" });
    expect(within(legacyRow).getByTitle("Model —; Reasoning effort —")).toHaveTextContent(/^—$/);

    const missingEffortRow = screen.getByRole("button", {
      name: "POST api.example.test/missing-effort",
    });
    expect(
      within(missingEffortRow).getByTitle("Model gpt-5.6-sol; Reasoning effort —"),
    ).toHaveTextContent(/^gpt-5\.6-sol$/);

    const longTimingRow = screen.getByRole("button", {
      name: "POST api.example.test/long-timing",
    });
    expect(
      within(longTimingRow).getByTitle("First token 17m34s; Duration 28m55s"),
    ).toHaveTextContent("17m34s / 28m55s");
  });

  it("includes a list issue in the request row's accessible description", async () => {
    const message = "Our servers are currently overloaded. Please try again later.";
    const issueSummary = {
      ...completedSummary,
      assessment: {
        level: "error" as const,
        primary: { source: "provider" as const, kind: "server_error", message },
        issue_count: 1,
      },
    };
    renderApp({ listRequests: vi.fn().mockResolvedValue(requestListFor([issueSummary])) });

    const row = await screen.findByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    const issueMarker = within(row).getByRole("img", {
      name: /Request error: Server error.*currently overloaded/,
    });
    expect(issueMarker).not.toHaveAttribute("tabindex");
    expect(within(row).getByText("Error: Server error")).toBeInTheDocument();
    expect(within(row).queryByText("gpt-5.6-sol high")).not.toBeInTheDocument();
    expect(within(row).getByTitle("Model gpt-5.6-sol; Reasoning effort high")).toBeInTheDocument();
    expect(within(row).queryByText(message)).not.toBeInTheDocument();
    expect(row).toHaveAccessibleDescription(
      /Request error: Server error\. Our servers are currently overloaded/,
    );
    expect(row).toHaveAccessibleDescription(/Model gpt-5.6-sol; Reasoning effort high/);
  });

  it("does not repeat an HTTP status as a catalog issue label", async () => {
    const http401 = {
      ...completedSummary,
      status: 401,
      assessment: {
        level: "error" as const,
        primary: {
          source: "http" as const,
          kind: "http_401",
          message: "Upstream returned HTTP 401",
        },
        issue_count: 1,
      },
    };
    renderApp({ listRequests: vi.fn().mockResolvedValue(requestListFor([http401])) });

    const row = await screen.findByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    expect(within(row).getByText("401")).toBeInTheDocument();
    expect(within(row).queryByText("Error: HTTP 401")).not.toBeInTheDocument();
    expect(within(row).getByText("gpt-5.6-sol high")).toBeInTheDocument();
    expect(row).toHaveAccessibleDescription(/Request error: HTTP 401/);
  });

  it("keeps Refresh enabled while a background list load is pending", async () => {
    const pendingList = deferred<typeof requestList>();
    renderApp({
      listRequests: vi.fn().mockReturnValue(pendingList.promise),
    });

    expect(screen.getByRole("button", { name: /Refresh/ })).toBeEnabled();
    pendingList.resolve(requestList);
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
  });

  it("marks the list refresh busy until a manual refresh completes", async () => {
    const refresh = deferred<typeof requestList>();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockReturnValueOnce(refresh.promise);
    const user = userEvent.setup();
    renderApp({ listRequests });

    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
    const refreshButton = screen.getByRole("button", {
      name: "Refresh Requests",
    });
    await user.click(refreshButton);

    expect(screen.getByRole("button", { name: "Refreshing Requests" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refreshing Requests" })).toHaveAttribute(
      "aria-busy",
      "true",
    );

    refresh.resolve(requestList);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh Requests" })).toBeEnabled(),
    );
  });

  it("keeps Next clickable while a background list refresh is pending", async () => {
    vi.useFakeTimers();
    const firstPage = { ...requestList, has_next: true };
    const secondPage = requestListFor([completedSummary], { total: 73, deletable_count: 72 });
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockImplementationOnce(
        (_page, signal) =>
          new Promise((_resolve, reject) => {
            signal?.addEventListener("abort", () =>
              reject(new DOMException("Aborted", "AbortError")),
            );
          }),
      )
      .mockResolvedValueOnce(secondPage);
    renderApp({ listRequests });

    await flushEffects();
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    await advanceTimers(5000);
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: /Next/ }));
    await flushEffects();
    expect(screen.getByText(/^Page 2 of /)).toBeInTheDocument();
    expect(listRequests).toHaveBeenLastCalledWith(2, expect.any(AbortSignal));
  });

  it("aborts and ignores an older list response after a refresh", async () => {
    const initial = deferred<typeof requestList>();
    const refreshed = requestListFor([completedSummary]);
    let initialSignal: AbortSignal | undefined;
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockImplementationOnce((_page, signal) => {
        initialSignal = signal;
        return initial.promise;
      })
      .mockResolvedValueOnce(refreshed);
    const api = requestsApiFake({ listRequests });
    const replacementApi = requestsApiFake({ listRequests: vi.fn().mockResolvedValue(refreshed) });
    const { rerender } = render(<RequestsHarness api={api} />);

    await flushEffects();
    rerender(<RequestsHarness api={replacementApi} />);
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
    expect(initialSignal?.aborted).toBe(true);

    await act(async () => {
      initial.resolve(requestList);
      await initial.promise;
    });
    expect(
      screen.getByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "GET stream.example.test/events" }),
    ).not.toBeInTheDocument();
  });

  it("falls back when polling finds the current page empty", async () => {
    vi.useFakeTimers();
    const secondPageSummary = completedSummaryFor("0198-demo-poll-page", "poll.example.test");
    const firstPage = { ...requestList, has_next: true };
    const secondPage = requestListFor([secondPageSummary], { total: 51, deletable_count: 50 });
    const emptySecondPage = requestListFor([], { total: 50, deletable_count: 49 });
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPage);
    renderApp({ listRequests });

    await flushEffects();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await flushEffects();
    expect(screen.getByText(/^Page 2 of /)).toBeInTheDocument();

    await advanceTimers(5000);

    expect(screen.getByText(/^Page 1 of /)).toBeInTheDocument();
    expect(listRequests).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });
});
