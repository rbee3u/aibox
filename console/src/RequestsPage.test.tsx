import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCallback, useEffect, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RequestsPage } from "./RequestsPage";
import { ApiError } from "./controlApi";
import {
  activeDetail,
  activeRequestList,
  activeSummary,
  completedDetail,
  completedSummary,
  completedSummaryFor,
  deferred,
  fakeApi,
  requestList,
  requestListFor,
  withIncompleteRequestBody,
  withRequestEncoding,
} from "./test/fixtures";
import type { RequestsApi } from "./types";

const zstdBytes = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);

function RequestsHarness({ api }: { api: RequestsApi }) {
  const [search, setSearch] = useState(window.location.search);
  useEffect(() => {
    const readLocation = () => setSearch(window.location.search);
    window.addEventListener("popstate", readLocation);
    return () => window.removeEventListener("popstate", readLocation);
  }, []);
  const onLocationChange = useCallback((query: URLSearchParams, replace = false) => {
    const suffix = query.toString();
    const next = `${window.location.pathname}${suffix ? `?${suffix}` : ""}`;
    window.history[replace ? "replaceState" : "pushState"](null, "", next);
    setSearch(suffix ? `?${suffix}` : "");
  }, []);
  return <RequestsPage api={api} search={search} onLocationChange={onLocationChange} />;
}

function renderApp(overrides: Partial<RequestsApi> = {}) {
  return render(<RequestsHarness api={fakeApi(overrides)} />);
}

function flushEffects() {
  return act(async () => Promise.resolve());
}

function advanceTimers(milliseconds: number) {
  return act(async () => vi.advanceTimersByTimeAsync(milliseconds));
}

async function selectCompletedRecord(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Select" }));
  await user.click(
    screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
  );
}

async function openCompletedRecord(user: ReturnType<typeof userEvent.setup>) {
  await user.click(
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
  );
}

async function openActiveRecord() {
  await flushEffects();
  fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
  await flushEffects();
}

async function openActiveRequestBody() {
  await openActiveRecord();
  fireEvent.click(screen.getByRole("tab", { name: "Request" }));
  await flushEffects();
}

async function confirmDeletion(
  user: ReturnType<typeof userEvent.setup>,
  action: "Delete selected",
) {
  await user.click(screen.getByRole("button", { name: action }));
  await user.click(screen.getByRole("button", { name: "Delete permanently" }));
}

async function confirmSingleDeletion(user: ReturnType<typeof userEvent.setup>, buttonName: string) {
  await user.click(await screen.findByRole("button", { name: buttonName }));
  expect(screen.getByRole("dialog", { name: "Delete this Request?" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Delete permanently" }));
}

afterEach(() => {
  vi.useRealTimers();
  window.history.replaceState(null, "", "/");
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("Requests page", () => {
  it("shows an explicit loading state before the first list response", async () => {
    const pending = deferred<typeof requestList>();
    renderApp({ listRequests: vi.fn().mockReturnValue(pending.promise) });

    expect(screen.getByRole("status")).toHaveTextContent("Loading Requests…");
    expect(screen.queryByText("No request recorded yet.")).not.toBeInTheDocument();

    pending.resolve(requestListFor([]));
    const emptyList = await screen.findByText("No request recorded yet.");
    expect(emptyList.closest('[data-empty-state="list"]')).toBeInTheDocument();
  });

  it("normalizes invalid Request URL state with replaceState", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/requests?page=2oops&request=%20&tab=body&extra=retired",
    );
    const replaceState = vi.spyOn(window.history, "replaceState");
    const listRequests = vi.fn<RequestsApi["listRequests"]>().mockResolvedValue(requestList);

    renderApp({ listRequests });

    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
    expect(listRequests).toHaveBeenCalledWith(1, expect.any(AbortSignal));
    expect(window.location.pathname).toBe("/_aibox/ui/requests");
    expect(window.location.search).toBe("");
    expect(replaceState).toHaveBeenCalled();
  });

  it("restores page, Request, and detail Tab from the URL", async () => {
    window.history.replaceState(
      null,
      "",
      `/_aibox/ui/requests?page=2&request=${completedSummary.id}&tab=response`,
    );
    const listRequests = vi.fn<RequestsApi["listRequests"]>().mockResolvedValue(requestList);
    const getRequest = vi.fn<RequestsApi["getRequest"]>().mockResolvedValue(completedDetail);

    renderApp({ listRequests, getRequest });

    expect(await screen.findByRole("tab", { name: "Response" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(listRequests).toHaveBeenCalledWith(2, expect.any(AbortSignal));
    expect(getRequest).toHaveBeenCalledWith(completedSummary.id, expect.any(AbortSignal));
    expect(window.location.search).toBe(`?page=2&request=${completedSummary.id}&tab=response`);
  });

  it("updates URL state while paging, opening a request, and changing Tabs", async () => {
    const user = userEvent.setup();
    renderApp({
      listRequests: vi.fn().mockResolvedValue({ ...requestList, has_next: true }),
    });

    await openCompletedRecord(user);
    expect(window.location.search).toBe(`?request=${completedSummary.id}`);
    await user.click(screen.getByRole("tab", { name: "Request" }));
    expect(window.location.search).toBe(`?request=${completedSummary.id}&tab=request`);
    await user.click(screen.getByRole("button", { name: "Back to Request list" }));
    expect(window.location.search).toBe("");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(window.location.search).toBe("?page=2");
  });

  it("does not navigate when clicking the already active Request detail Tab", async () => {
    const user = userEvent.setup();
    const pushState = vi.spyOn(window.history, "pushState");
    renderApp();

    await openCompletedRecord(user);
    pushState.mockClear();

    await user.click(screen.getByRole("tab", { name: "Summary" }));
    expect(pushState).not.toHaveBeenCalled();

    await user.click(screen.getByRole("tab", { name: "Request" }));
    expect(pushState).toHaveBeenCalledTimes(1);
    pushState.mockClear();

    await user.click(screen.getByRole("tab", { name: "Request" }));
    expect(pushState).not.toHaveBeenCalled();

    await user.click(screen.getByRole("tab", { name: "Response" }));
    expect(pushState).toHaveBeenCalledTimes(1);
    pushState.mockClear();

    await user.click(screen.getByRole("tab", { name: "Response" }));
    expect(pushState).not.toHaveBeenCalled();
  });

  it("retries a failed page navigation and keeps the route aligned with the visible page", async () => {
    const secondPageSummary = completedSummaryFor(
      "0198-demo-page-navigation-retry",
      "second.example.test",
    );
    const firstPage = requestListFor([completedSummary], {
      total: 51,
      deletable_count: 51,
      has_next: true,
    });
    const secondPage = requestListFor([secondPageSummary], {
      total: 51,
      deletable_count: 51,
    });
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockRejectedValueOnce(new Error("second page unavailable"))
      .mockResolvedValueOnce(secondPage);
    const user = userEvent.setup();
    renderApp({ listRequests });

    await user.click(await screen.findByRole("button", { name: "Next" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("second page unavailable");
    expect(window.location.search).toBe("");
    expect(screen.getByText("Page 1 of 2 · 1 shown · 51 total")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();

    await user.click(within(alert).getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("button", { name: "POST second.example.test/v1/responses" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 2 of 2 · 1 shown · 51 total")).toBeInTheDocument();
    expect(window.location.search).toBe("?page=2");
    expect(listRequests.mock.calls.map(([page]) => page)).toEqual([1, 2, 2]);
  });

  it("restores the URL Tab when browser history changes the selected request", async () => {
    const secondSummary = completedSummaryFor("0198-demo-completed-second", "second.example.test");
    const secondDetail = {
      ...completedDetail,
      request: {
        ...completedDetail.request,
        id: secondSummary.id,
        upstream_url: secondSummary.upstream_url,
      },
    };
    const user = userEvent.setup();
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockImplementation((id) =>
        Promise.resolve(id === secondSummary.id ? secondDetail : completedDetail),
      );
    renderApp({
      listRequests: vi.fn().mockResolvedValue(requestListFor([completedSummary, secondSummary])),
      getRequest,
    });

    await user.click(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("tab", { name: "Response" }));
    await user.click(screen.getByRole("button", { name: "POST second.example.test/v1/responses" }));

    expect(window.location.search).toBe(`?request=${secondSummary.id}`);
    act(() => window.history.back());

    await waitFor(() => {
      expect(window.location.search).toBe(`?request=${completedSummary.id}&tab=response`);
      expect(screen.getByRole("tab", { name: "Response" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });
  });

  it("returns to the list and clears a stale Request URL", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/requests?page=2&request=missing-request&tab=response",
    );
    renderApp({
      listRequests: vi.fn().mockResolvedValue(requestList),
      getRequest: vi.fn().mockRejectedValue(new ApiError("Request not found", 404)),
    });

    expect(await screen.findByRole("alert")).toHaveTextContent("Request not found");
    expect(window.location.search).toBe("?page=2");
    expect(screen.getByRole("heading", { level: 2, name: "Select a Request" })).toBeInTheDocument();
    expect(
      screen
        .getByRole("heading", { level: 2, name: "Select a Request" })
        .closest('[data-empty-state="detail"]'),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("renders concise Request summaries in the Console module", async () => {
    renderApp();

    const requestListPanel = await screen.findByRole("complementary", {
      name: "Request list",
    });
    expect(within(requestListPanel).queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
    expect(
      within(requestListPanel).getByRole("button", { name: "Refresh Request list" }),
    ).toBeEnabled();
    expect(within(requestListPanel).getByRole("button", { name: "Select" })).toBeEnabled();
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

  it("ignores the retired list-width preference and exposes no resize control", () => {
    window.localStorage.setItem("aibox-request-list-width", "640");
    renderApp();

    expect(screen.queryByRole("separator")).not.toBeInTheDocument();
    expect(window.localStorage.getItem("aibox-request-list-width")).toBe("640");
  });

  it("returns from a pending inspection without changing history and restores row focus", async () => {
    const detailRequest = deferred<typeof completedDetail>();
    let detailSignal: AbortSignal | undefined;
    const getRequest = vi.fn<RequestsApi["getRequest"]>().mockImplementation((_id, signal) => {
      detailSignal = signal;
      return detailRequest.promise;
    });
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/_aibox/ui/requests");
    renderApp({ getRequest });

    const row = await screen.findByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    await user.click(row);
    expect(screen.getByText("Loading request…")).toBeInTheDocument();
    expect(window.location.pathname).toBe("/_aibox/ui/requests");

    await user.click(screen.getByRole("button", { name: "Back to Request list" }));

    expect(detailSignal?.aborted).toBe(true);
    expect(screen.getByRole("heading", { name: "Select a Request" })).toBeInTheDocument();
    expect(document.querySelector('[data-icon="request-detail-empty"]')).toHaveClass(
      "lucide-arrow-left-right",
    );
    await waitFor(() => expect(row).toHaveFocus());
    expect(window.location.pathname).toBe("/_aibox/ui/requests");

    detailRequest.resolve(completedDetail);
    await flushEffects();
    expect(screen.queryByRole("region", { name: "Request details" })).not.toBeInTheDocument();
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
    expect(within(row).queryByText("Server error")).not.toBeInTheDocument();
    expect(row).toHaveAccessibleDescription(
      /Request error: Server error\. Our servers are currently overloaded/,
    );
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
      name: "Refresh Request list",
    });
    await user.click(refreshButton);

    expect(screen.getByRole("button", { name: "Refreshing Request list" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refreshing Request list" })).toHaveAttribute(
      "aria-busy",
      "true",
    );

    refresh.resolve(requestList);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh Request list" })).toBeEnabled(),
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

  it("switches from browsing controls to selection controls", async () => {
    const user = userEvent.setup();
    renderApp();

    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select page" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: "Cannot delete active GET stream.example.test/events",
      }),
    ).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Select" }));
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const count = screen.getByText("0 selected");
    const pageSelection = screen.getByRole("button", { name: "Select page" });
    const deleteSelected = screen.getByRole("button", { name: "Delete selected" });
    expect(cancel.compareDocumentPosition(count) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      count.compareDocumentPosition(pageSelection) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      pageSelection.compareDocumentPosition(deleteSelected) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByText("0 selected")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh Request list" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete selected" })).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Select GET stream.example.test/events",
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("selects and clears all completed requests on the current page", async () => {
    const secondCompleted = completedSummaryFor(
      "0198-demo-completed-second",
      "second.example.test",
    );
    const user = userEvent.setup();
    renderApp({
      listRequests: vi
        .fn()
        .mockResolvedValue(requestListFor([activeSummary, completedSummary, secondCompleted])),
    });

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select page" }));

    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect POST second.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Clear page" }));

    expect(screen.getByText("0 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete selected" })).toBeDisabled();
  });

  it("disables selection when no completed requests exist", async () => {
    renderApp({ listRequests: vi.fn().mockResolvedValue(activeRequestList) });

    expect(await screen.findByRole("button", { name: "Select" })).toBeDisabled();
  });

  it("confirms one-request deletion, locks deletion, clears detail, and restores focus", async () => {
    const secondCompleted = completedSummaryFor(
      "0198-demo-completed-second",
      "second.example.test",
    );
    const initial = requestListFor([completedSummary, secondCompleted]);
    const afterDelete = requestListFor([secondCompleted]);
    const deleteRequest = deferred<number>();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(initial)
      .mockResolvedValue(afterDelete);
    const deleteRequests = vi
      .fn<RequestsApi["deleteRequests"]>()
      .mockReturnValue(deleteRequest.promise);
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests });

    await openCompletedRecord(user);
    await screen.findByRole("region", { name: "Request details" });
    await user.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );
    expect(screen.getByRole("dialog", { name: "Delete this Request?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));
    const deleting = screen.getByRole("button", {
      name: "Deleting POST api.example.test/v1/responses",
    });
    expect(deleting).toBeDisabled();
    expect(deleting).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select" })).toBeDisabled();

    act(() => deleteRequest.resolve(1));

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
      ).not.toBeInTheDocument(),
    );
    expect(deleteRequests).toHaveBeenCalledWith([completedSummary.id]);
    expect(window.location.search).toBe("");
    expect(screen.getByText("Page 1 of 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a Request" })).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
      ).toHaveFocus(),
    );
    expect(screen.queryByText("Request deleted")).not.toBeInTheDocument();
  });

  it("keeps list navigation locked while a request deletion is pending", async () => {
    vi.useFakeTimers();
    const firstPage = requestListFor(requestList.requests, {
      total: 51,
      deletable_count: 50,
      has_next: true,
    });
    const deleteRequest = deferred<number>();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValue(requestListFor([activeSummary], { total: 50, deletable_count: 49 }));
    renderApp({
      listRequests,
      deleteRequests: vi.fn().mockReturnValue(deleteRequest.promise),
    });

    await flushEffects();
    fireEvent.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(screen.getByRole("button", { name: "Refresh Request list" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    await advanceTimers(10_000);
    expect(listRequests).toHaveBeenCalledTimes(1);

    await act(async () => {
      deleteRequest.resolve(1);
      await deleteRequest.promise;
    });
    expect(listRequests.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByRole("button", { name: "Refresh Request list" })).toBeEnabled();
  });

  it("keeps a request when immediate deletion fails", async () => {
    const deleteRequests = vi
      .fn<RequestsApi["deleteRequests"]>()
      .mockRejectedValue(new Error("cannot delete request"));
    const user = userEvent.setup();
    renderApp({ deleteRequests });

    await confirmSingleDeletion(user, "Delete POST api.example.test/v1/responses");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("cannot delete request");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toBeEnabled();
    await user.click(within(alert).getByRole("button", { name: "Dismiss message" }));
    expect(alert).not.toBeInTheDocument();
  });

  it("keeps an immediately deleted request removed when its list refresh fails", async () => {
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("cannot refresh Requests"));
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests: vi.fn().mockResolvedValue(1) });

    await confirmSingleDeletion(user, "Delete POST api.example.test/v1/responses");

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot refresh Requests");
    expect(
      screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 of 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh Request list" })).toHaveFocus();
  });

  it("returns to the previous page when immediate deletion empties the current page", async () => {
    const secondPageSummary = completedSummaryFor("0198-demo-second-page", "second.example.test");
    const firstPage = requestListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = requestListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const emptySecondPage = requestListFor([], { total: 1, deletable_count: 1 });
    const firstPageAfterDelete = requestListFor([completedSummary]);
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPageAfterDelete);
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests: vi.fn().mockResolvedValue(1) });

    await user.click(await screen.findByRole("button", { name: "Next" }));
    await confirmSingleDeletion(user, "Delete POST second.example.test/v1/responses");

    await screen.findByText("Page 1 of 1 · 1 shown · 1 total");
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toHaveFocus();
  });

  it("preserves the selected count across request pages", async () => {
    const secondPageSummary = completedSummaryFor("0198-demo-second-page", "second.example.test");
    const firstPage = requestListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = requestListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage);
    const user = userEvent.setup();
    renderApp({ listRequests });

    await selectCompletedRecord(user);
    await user.click(screen.getByRole("button", { name: /Next/ }));

    await screen.findByRole("button", { name: "Select POST second.example.test/v1/responses" });
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Select POST second.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("returns to the first page after deleting a selection across pages", async () => {
    const secondPageSummary = completedSummaryFor("0198-demo-second-page", "second.example.test");
    const firstPage = requestListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = requestListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const afterDelete = requestListFor([]);
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValue(afterDelete);
    const deleteRequests = vi.fn<RequestsApi["deleteRequests"]>().mockResolvedValue(2);
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests });

    await selectCompletedRecord(user);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Select POST second.example.test/v1/responses",
      }),
    );
    await confirmDeletion(user, "Delete selected");

    await screen.findByText("Page 1 of 1 · 0 shown · 0 total");
    expect(screen.getByText("No request recorded yet.")).toBeInTheDocument();
    expect(document.querySelector('[data-icon="request-empty"]')).toHaveClass("lucide-inbox");
    expect(deleteRequests).toHaveBeenCalledWith([completedSummary.id, secondPageSummary.id]);
    expect(listRequests).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });

  it("returns to the lowest selected page when selection starts on a later page", async () => {
    const page2Summary = completedSummaryFor("0198-demo-page-two", "two.example.test");
    const page3Summary = completedSummaryFor("0198-demo-page-three", "three.example.test");
    const pages = new Map([
      [1, requestListFor([completedSummary], { total: 3, deletable_count: 3, has_next: true })],
      [2, requestListFor([page2Summary], { total: 3, deletable_count: 3, has_next: true })],
      [3, requestListFor([page3Summary], { total: 3, deletable_count: 3 })],
    ]);
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockImplementation((page = 1) => Promise.resolve(pages.get(page)!));
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests: vi.fn().mockResolvedValue(2) });

    await user.click(await screen.findByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST three.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("button", { name: "Previous" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST two.example.test/v1/responses" }),
    );
    await confirmDeletion(user, "Delete selected");

    await waitFor(() => expect(listRequests).toHaveBeenLastCalledWith(2, expect.any(AbortSignal)));
    expect(screen.getByText(/^Page 2 of /)).toBeInTheDocument();
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

  it("clears selection on Cancel, keeps focus safe, and ignores Escape", async () => {
    const user = userEvent.setup();
    renderApp();

    await selectCompletedRecord(user);
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await user.click(cancel);

    expect(screen.getByRole("button", { name: "Refresh Request list" })).toBeInTheDocument();
    const select = screen.getByRole("button", { name: "Select" });
    expect(select).toHaveFocus();
    await user.click(select);
    expect(screen.getByText("0 selected")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("closes confirmation on Escape without clearing the selection", async () => {
    const user = userEvent.setup();
    renderApp();

    await selectCompletedRecord(user);
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected Request?" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(dialog).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("closes confirmation but keeps selection mode and selected ids when deletion fails", async () => {
    const user = userEvent.setup();
    renderApp({ deleteRequests: vi.fn().mockRejectedValue(new Error("delete failed")) });

    await selectCompletedRecord(user);
    await confirmDeletion(user, "Delete selected");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t delete request");
    expect(alert).toHaveTextContent("delete failed");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect POST api.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("pauses list polling during selection and refreshes immediately after exit", async () => {
    vi.useFakeTimers();
    const listRequests = vi.fn<RequestsApi["listRequests"]>().mockResolvedValue(requestList);
    renderApp({ listRequests });

    await flushEffects();
    expect(screen.getByRole("button", { name: "Select" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    await advanceTimers(7500);

    expect(listRequests).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Refresh Request list" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await flushEffects();
    expect(listRequests).toHaveBeenCalledTimes(2);
  });

  it("pauses inspection streams while selected deletion confirmation is open", async () => {
    vi.useFakeTimers();
    const listPoll = deferred<typeof requestList>();
    const detailPoll = deferred<typeof activeDetail>();
    const bodyPoll = deferred<{ bytes: Uint8Array; nextOffset: number }>();
    let listSignal: AbortSignal | undefined;
    let detailSignal: AbortSignal | undefined;
    let bodySignal: AbortSignal | undefined;
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockImplementationOnce((_page, signal) => {
        listSignal = signal;
        return listPoll.promise;
      })
      .mockResolvedValue(requestList);
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockImplementationOnce((_id, signal) => {
        detailSignal = signal;
        return detailPoll.promise;
      })
      .mockResolvedValue(activeDetail);
    const loadBody = vi
      .fn<RequestsApi["loadBody"]>()
      .mockImplementationOnce((_id, _kind, _offset, signal) => {
        bodySignal = signal;
        return bodyPoll.promise;
      })
      .mockResolvedValue({ bytes: new Uint8Array(), nextOffset: 0 });
    renderApp({ listRequests, getRequest, loadBody });

    await openActiveRecord();
    fireEvent.click(screen.getByRole("tab", { name: "Request" }));
    await flushEffects();
    await advanceTimers(3000);
    await advanceTimers(2000);
    expect([listSignal?.aborted, detailSignal?.aborted, bodySignal?.aborted]).toEqual([
      false,
      false,
      false,
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    await flushEffects();
    expect([listSignal?.aborted, detailSignal?.aborted, bodySignal?.aborted]).toEqual([
      true,
      false,
      false,
    ]);
    fireEvent.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected Request?" });
    expect(dialog).toHaveTextContent(/selected raw Request and Response data/i);
    await flushEffects();
    expect([listSignal?.aborted, detailSignal?.aborted, bodySignal?.aborted]).toEqual([
      true,
      true,
      true,
    ]);
    await advanceTimers(10_000);
    expect(listRequests).toHaveBeenCalledTimes(2);
    expect(getRequest).toHaveBeenCalledTimes(2);
    expect(loadBody).toHaveBeenCalledTimes(1);

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await flushEffects();
    expect(listRequests).toHaveBeenCalledTimes(2);
    expect(getRequest).toHaveBeenCalledTimes(3);
    expect(loadBody).toHaveBeenCalledTimes(2);
  });

  it("loads request and response Bodies only when their tabs are selected", async () => {
    const encoder = new TextEncoder();
    const loadBody = vi.fn<RequestsApi["loadBody"]>().mockImplementation((_id, kind) =>
      Promise.resolve({
        bytes: encoder.encode(kind === "request" ? "request body" : "data: response body\n\n"),
        nextOffset: kind === "request" ? 12 : 21,
      }),
    );
    const loadEventTimings = vi.fn<RequestsApi["loadEventTimings"]>().mockResolvedValue({
      state: "available",
      events: [{ sequence: 0, completed_at_ns: "900000000" }],
      next_sequence: 1,
      warning: null,
    });
    const user = userEvent.setup();
    renderApp({ loadBody, loadEventTimings });

    await openCompletedRecord(user);
    const detail = screen.getByRole("region", { name: "Request details" });
    expect(within(detail).getByRole("tab", { name: "Summary" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(loadBody).not.toHaveBeenCalled();
    await user.click(within(detail).getByRole("tab", { name: "Request" }));
    await screen.findByText("request body");
    await user.click(within(detail).getByRole("tab", { name: "Response" }));
    await user.click(await screen.findByRole("button", { name: /message/ }));
    expect(screen.getByText("response body")).toBeInTheDocument();
    expect(loadEventTimings).toHaveBeenCalledWith(completedSummary.id, 0, expect.any(AbortSignal));
  });

  it("keeps the detail view and retries a body read failure from its current offset", async () => {
    const loadBody = vi
      .fn<RequestsApi["loadBody"]>()
      .mockRejectedValueOnce(new Error("body unavailable"))
      .mockResolvedValue({
        bytes: new TextEncoder().encode("recovered body"),
        nextOffset: 14,
      });
    const user = userEvent.setup();
    renderApp({ loadBody });

    await openCompletedRecord(user);
    await user.click(await screen.findByRole("tab", { name: "Request" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t load Body");
    expect(alert).toHaveTextContent("body unavailable");
    const detail = screen.getByRole("region", { name: "Request details" });
    expect(within(detail).getByRole("status")).toHaveTextContent("Original Body unavailable.");
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("recovered body")).toBeInTheDocument();
    expect(loadBody).toHaveBeenLastCalledWith(
      completedSummary.id,
      "request",
      0,
      expect.any(AbortSignal),
    );
  });

  it("retries downloading the same Body after a download failure", async () => {
    const bytes = new TextEncoder().encode("body");
    const loadBody = vi
      .fn<RequestsApi["loadBody"]>()
      .mockResolvedValueOnce({ bytes, nextOffset: bytes.length })
      .mockRejectedValueOnce(new Error("download unavailable"))
      .mockResolvedValue({ bytes, nextOffset: bytes.length });
    const createObjectURL = vi.fn().mockReturnValue("blob:test");
    const NativeURL = URL;
    class TestURL extends NativeURL {
      static createObjectURL = createObjectURL;
      static revokeObjectURL = vi.fn();
    }
    vi.stubGlobal("URL", TestURL);
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const user = userEvent.setup();
    renderApp({ loadBody });

    await openCompletedRecord(user);
    await user.click(await screen.findByRole("tab", { name: "Request" }));
    await screen.findByText("body");
    await user.click(screen.getByRole("button", { name: "Download original body" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t download Body");
    expect(alert).toHaveTextContent("download unavailable");

    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1));
    expect(loadBody).toHaveBeenLastCalledWith(
      completedSummary.id,
      "request",
      0,
      expect.any(AbortSignal),
    );
  });

  it("loads zstd decoded Source only after the complete raw Body is available", async () => {
    const decoded = new TextEncoder().encode('{"model":"gpt-5.6-sol"}');
    const detail = {
      ...withRequestEncoding(completedDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const loadDecodedBody = vi.fn<RequestsApi["loadDecodedBody"]>().mockResolvedValue(decoded);
    renderApp({
      getRequest: vi.fn().mockResolvedValue(detail),
      loadBody: vi.fn().mockResolvedValue({ bytes: zstdBytes, nextOffset: zstdBytes.length }),
      loadDecodedBody,
    });
    const user = userEvent.setup();

    await openCompletedRecord(user);
    await user.click(screen.getByRole("tab", { name: "Request" }));

    await screen.findByText('"gpt-5.6-sol"');
    expect(loadDecodedBody).toHaveBeenCalledWith(
      completedSummary.id,
      "request",
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("button", { name: "Pretty" })).toHaveAttribute("aria-pressed", "true");
  });

  it("clears a previous zstd decode error while retrying", async () => {
    const detail = {
      ...withRequestEncoding(completedDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const retry = deferred<Uint8Array>();
    const loadDecodedBody = vi
      .fn<RequestsApi["loadDecodedBody"]>()
      .mockRejectedValueOnce(new Error("decode failed"))
      .mockReturnValueOnce(retry.promise);
    const user = userEvent.setup();
    renderApp({
      getRequest: vi.fn().mockResolvedValue(detail),
      loadBody: vi.fn().mockImplementation((_id, _kind, offset) =>
        Promise.resolve({
          bytes: offset === 0 ? zstdBytes : new Uint8Array(),
          nextOffset: zstdBytes.length,
        }),
      ),
      loadDecodedBody,
    });

    await openCompletedRecord(user);
    await user.click(screen.getByRole("tab", { name: "Request" }));
    await screen.findByText(/Decoded Source unavailable: decode failed/);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Summary" }));
    await user.click(screen.getByRole("tab", { name: "Request" }));

    await screen.findByText(/Decoding zstd Body/);
    expect(screen.queryByText(/decode failed/)).not.toBeInTheDocument();
    retry.resolve(new TextEncoder().encode('{"state":"ready"}'));
  });

  it("does not restart a slow zstd decode for unchanged active detail polls", async () => {
    vi.useFakeTimers();
    const decoded = new TextEncoder().encode('{"state":"ready"}');
    const zstdDetail = {
      ...withRequestEncoding(activeDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const getRequest = vi.fn<RequestsApi["getRequest"]>().mockImplementation(() =>
      Promise.resolve({
        ...zstdDetail,
        request: { ...zstdDetail.request },
        summary: { ...zstdDetail.summary },
      }),
    );
    const loadBody = vi.fn<RequestsApi["loadBody"]>().mockImplementation((_id, _kind, offset) =>
      Promise.resolve({
        bytes: offset === 0 ? zstdBytes : new Uint8Array(),
        nextOffset: zstdBytes.length,
      }),
    );
    let decodedSignal: AbortSignal | undefined;
    const decodedRequest = deferred<Uint8Array>();
    const loadDecodedBody = vi
      .fn<RequestsApi["loadDecodedBody"]>()
      .mockImplementation((_id, _kind, signal) => {
        decodedSignal = signal;
        return decodedRequest.promise;
      });
    renderApp({
      listRequests: vi.fn().mockResolvedValue(activeRequestList),
      getRequest,
      loadBody,
      loadDecodedBody,
    });

    await openActiveRequestBody();
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);

    await advanceTimers(3000);
    expect(getRequest).toHaveBeenCalledTimes(2);
    expect(loadBody).toHaveBeenCalledTimes(1);
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);
    expect(decodedSignal?.aborted).toBe(false);

    await act(async () => {
      decodedRequest.resolve(decoded);
      await decodedRequest.promise;
    });
    expect(screen.getByText('"ready"')).toBeInTheDocument();
  });

  it("ignores a stale zstd decode failure after selecting another request", async () => {
    const decoded = new TextEncoder().encode('{"state":"ready"}');
    const completedZstd = {
      ...withRequestEncoding(completedDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const activeZstd = {
      ...withRequestEncoding(activeDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const firstDecode = deferred<Uint8Array>();
    const secondDecode = deferred<Uint8Array>();
    const decodeSignals: AbortSignal[] = [];
    const loadDecodedBody = vi
      .fn<RequestsApi["loadDecodedBody"]>()
      .mockImplementation((_id, _kind, signal) => {
        decodeSignals.push(signal!);
        return decodeSignals.length === 1 ? firstDecode.promise : secondDecode.promise;
      });
    renderApp({
      getRequest: vi
        .fn()
        .mockImplementation((id: string) =>
          Promise.resolve(id === completedSummary.id ? completedZstd : activeZstd),
        ),
      loadBody: vi.fn().mockResolvedValue({ bytes: zstdBytes, nextOffset: zstdBytes.length }),
      loadDecodedBody,
    });
    const user = userEvent.setup();

    await openCompletedRecord(user);
    await user.click(screen.getByRole("tab", { name: "Request" }));
    await waitFor(() => expect(loadDecodedBody).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
    await user.click(await screen.findByRole("tab", { name: "Request" }));
    await waitFor(() => expect(loadDecodedBody).toHaveBeenCalledTimes(2));
    expect(decodeSignals.map((signal) => signal.aborted)).toEqual([true, false]);

    await act(async () => {
      firstDecode.reject(new Error("stale decode failed"));
      await Promise.resolve();
    });
    expect(screen.queryByText(/stale decode failed/)).not.toBeInTheDocument();

    await act(async () => {
      secondDecode.resolve(decoded);
      await secondDecode.promise;
    });
    expect(screen.getByText('"ready"')).toBeInTheDocument();
  });

  it("clears a pending completed inspection when selected deletion includes it", async () => {
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockResolvedValue(activeRequestList);
    const detailRequest = deferred<typeof completedDetail>();
    const user = userEvent.setup();
    renderApp({
      listRequests,
      getRequest: vi.fn().mockReturnValue(detailRequest.promise),
      deleteRequests: vi.fn().mockResolvedValue(1),
    });

    await openCompletedRecord(user);
    expect(screen.getByText("Loading request…")).toBeInTheDocument();
    await selectCompletedRecord(user);
    await confirmDeletion(user, "Delete selected");

    await screen.findByRole("heading", { name: "Select a Request" });
    expect(screen.queryByText("Loading request…")).not.toBeInTheDocument();
    expect(window.location.search).toBe("");
  });

  it("shows list failures and retries in place", async () => {
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockRejectedValueOnce(new Error("cannot scan Requests"))
      .mockResolvedValue(requestList);
    const user = userEvent.setup();
    renderApp({ listRequests });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("cannot scan Requests");
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
  });

  it("retries the currently selected request from its inspection notification", async () => {
    window.history.replaceState(
      null,
      "",
      `/_aibox/ui/requests?request=${completedSummary.id}&tab=response`,
    );
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockRejectedValueOnce(new Error("detail unavailable"))
      .mockResolvedValue(completedDetail);
    const user = userEvent.setup();
    renderApp({ getRequest });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t load request");
    expect(screen.getByRole("heading", { name: "Request unavailable" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to Request list" })).toBeInTheDocument();
    await user.click(within(alert).getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("region", { name: "Request details" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Response" })).toHaveAttribute("aria-selected", "true");
    expect(window.location.search).toBe(`?request=${completedSummary.id}&tab=response`);
    expect(getRequest).toHaveBeenCalledTimes(2);
  });

  it("clears a request that disappears before its detail loads", async () => {
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockRejectedValue(new ApiError("Request not found", 404));
    const user = userEvent.setup();
    renderApp({ getRequest });

    await openCompletedRecord(user);

    await screen.findByRole("heading", { name: "Select a Request" });
    expect(screen.getByRole("alert")).toHaveTextContent("Request not found");
  });

  it("stacks simultaneous list and detail failures by source", async () => {
    vi.useFakeTimers();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("list polling failed"));
    renderApp({
      listRequests,
      getRequest: vi.fn().mockRejectedValue(new Error("detail loading failed")),
    });

    await flushEffects();
    fireEvent.click(screen.getByRole("button", { name: "POST api.example.test/v1/responses" }));
    await flushEffects();
    expect(screen.getByRole("alert")).toHaveTextContent("detail loading failed");

    await advanceTimers(5000);
    expect(screen.getAllByRole("alert")).toHaveLength(2);
    expect(screen.getByText("list polling failed")).toBeInTheDocument();
    expect(screen.getByText("detail loading failed")).toBeInTheDocument();
  });

  it("orders the three notification sources with the newest failure first", async () => {
    vi.useFakeTimers();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("list failed"));
    renderApp({
      listRequests,
      getRequest: vi.fn().mockRejectedValue(new Error("detail failed")),
      deleteRequests: vi.fn().mockRejectedValue(new Error("delete failed")),
    });

    await flushEffects();
    fireEvent.click(screen.getByRole("button", { name: "POST api.example.test/v1/responses" }));
    await flushEffects();
    await advanceTimers(5000);
    fireEvent.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await flushEffects();

    expect(screen.getAllByRole("alert").map((alert) => alert.textContent)).toEqual([
      expect.stringContaining("Couldn’t load request"),
      expect.stringContaining("Couldn’t delete request"),
      expect.stringContaining("Couldn’t load requests"),
    ]);
  });

  it("does not re-notify a continuous polling failure until a successful request", async () => {
    vi.useFakeTimers();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("polling remains unavailable"));
    renderApp({ listRequests });

    await flushEffects();
    await advanceTimers(5000);
    expect(screen.getByRole("alert")).toHaveTextContent("polling remains unavailable");
    await advanceTimers(8000);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    listRequests
      .mockResolvedValueOnce(requestList)
      .mockRejectedValueOnce(new Error("polling remains unavailable"));
    await advanceTimers(2000);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await advanceTimers(5000);
    expect(screen.getByRole("alert")).toHaveTextContent("polling remains unavailable");
  });

  it("keeps a post-delete list refresh failure visible", async () => {
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("cannot refresh Requests"));
    const user = userEvent.setup();
    renderApp({ listRequests, deleteRequests: vi.fn().mockResolvedValue(1) });

    await selectCompletedRecord(user);
    await confirmDeletion(user, "Delete selected");

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot refresh Requests");
    expect(
      screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 of 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select" })).toBeDisabled();
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
    const api = fakeApi({ listRequests });
    const replacementApi = fakeApi({ listRequests: vi.fn().mockResolvedValue(refreshed) });
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

  it("polls an active detail until it completes", async () => {
    vi.useFakeTimers();
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockResolvedValueOnce({
        ...completedDetail,
        request: { ...completedDetail.request, id: activeSummary.id },
      });
    renderApp({
      listRequests: vi.fn().mockResolvedValue(activeRequestList),
      getRequest,
    });

    await openActiveRecord();
    const detail = screen.getByRole("region", { name: "Request details" });
    expect(within(detail).getAllByText("Waiting").length).toBeGreaterThan(0);

    await advanceTimers(3000);
    expect(getRequest).toHaveBeenCalledTimes(2);
    expect(within(detail).queryAllByText("Waiting")).toHaveLength(0);
    expect(within(detail).getByLabelText("HTTP/2 200 OK")).toBeInTheDocument();
  });

  it("stops polling an active detail after a deterministic not-found response", async () => {
    vi.useFakeTimers();
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockRejectedValue(new ApiError("Request not found", 404));
    renderApp({
      listRequests: vi.fn().mockResolvedValue(activeRequestList),
      getRequest,
    });

    await openActiveRecord();
    await advanceTimers(3000);

    expect(screen.getByRole("alert")).toHaveTextContent("Request not found");
    expect(screen.getByRole("heading", { name: "Select a Request" })).toBeInTheDocument();
    expect(getRequest).toHaveBeenCalledTimes(2);
    await advanceTimers(9000);
    expect(getRequest).toHaveBeenCalledTimes(2);
  });

  it("stops polling an active Body after a deterministic not-found response", async () => {
    vi.useFakeTimers();
    const incompleteDetail = withIncompleteRequestBody(activeDetail);
    const loadBody = vi
      .fn<RequestsApi["loadBody"]>()
      .mockRejectedValue(new ApiError("Request not found", 404));
    renderApp({
      listRequests: vi.fn().mockResolvedValue(activeRequestList),
      getRequest: vi.fn().mockResolvedValue(incompleteDetail),
      loadBody,
    });

    await openActiveRequestBody();

    expect(screen.getByRole("alert")).toHaveTextContent("Request not found");
    expect(screen.getByRole("region", { name: "Request details" })).toBeInTheDocument();
    expect(loadBody).toHaveBeenCalledTimes(1);
    await advanceTimers(9000);
    expect(loadBody).toHaveBeenCalledTimes(1);
  });

  it("does not overlap active body polls", async () => {
    vi.useFakeTimers();
    const encoder = new TextEncoder();
    const requestPoll = deferred<{ bytes: Uint8Array; nextOffset: number }>();
    const loadBody = vi.fn<RequestsApi["loadBody"]>().mockImplementation((_id, kind, offset) => {
      if (offset === 0) return Promise.resolve({ bytes: encoder.encode(kind), nextOffset: 1 });
      return requestPoll.promise;
    });
    const getRequest = vi
      .fn<RequestsApi["getRequest"]>()
      .mockResolvedValue(withIncompleteRequestBody(activeDetail));
    renderApp({
      listRequests: vi.fn().mockResolvedValue(activeRequestList),
      getRequest,
      loadBody,
    });

    await openActiveRequestBody();
    expect(loadBody).toHaveBeenCalledTimes(1);
    await advanceTimers(3000);
    expect(loadBody).toHaveBeenCalledTimes(2);

    await advanceTimers(3000);
    expect(loadBody).toHaveBeenCalledTimes(2);

    await act(async () => {
      requestPoll.resolve({ bytes: encoder.encode("next"), nextOffset: 5 });
      await Promise.resolve();
    });
    expect(loadBody).toHaveBeenCalledTimes(2);
  });
});
