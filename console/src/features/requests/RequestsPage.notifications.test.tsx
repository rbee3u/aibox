import { fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import { ApiError } from "@/api/requests";
import { completedDetail, completedSummary, requestList } from "@/features/requests/testFixtures";
import {
  advanceTimers,
  confirmDeletion,
  flushEffects,
  openCompletedRequest,
  renderApp,
  selectCompletedRequest,
} from "@/features/requests/testHarness";

describe("Requests page failure notifications", () => {
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

    await openCompletedRequest(user);

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

    await selectCompletedRequest(user);
    await confirmDeletion(user, "Delete selected");

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot refresh Requests");
    expect(
      screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 of 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Requests" })).toBeDisabled();
  });
});
