import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import { ApiError } from "@/api/transport";
import {
  completedDetail,
  completedSummary,
  completedSummaryFor,
  requestList,
  requestListFor,
} from "@/features/requests/testFixtures";
import { flushEffects, openCompletedRecord, renderApp } from "@/features/requests/testHarness";
import { deferred } from "@/test/deferred";

describe("Requests page routing", () => {
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
});
