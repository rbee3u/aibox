import { fireEvent, screen } from "@testing-library/react";
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

  it("retries the currently selected request from the detail pane that states the failure", async () => {
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

    // The pane carries the failure and its Retry, so nothing is said twice.
    expect(await screen.findByRole("heading", { name: "Request unavailable" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to Request list" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));

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

  it("keeps a list read failure inline while the detail pane states its own", async () => {
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
    expect(screen.getByRole("heading", { name: "Request unavailable" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    await advanceTimers(5000);
    // Each read states itself where its own content would be: one banner for
    // the list, the detail pane for the detail.
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent("list polling failed");
    expect(screen.getByRole("heading", { name: "Request unavailable" })).toBeInTheDocument();
  });

  it("notifies an action failure without repeating an inline read failure", async () => {
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
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await flushEffects();

    // Only the delete is an event; both reads already own a surface.
    const alerts = screen.getAllByRole("alert").map((alert) => alert.textContent);
    expect(alerts).toHaveLength(2);
    expect(alerts[0]).toContain("list failed");
    expect(alerts[1]).toContain("Couldn’t delete request");
    expect(screen.getByRole("heading", { name: "Request unavailable" })).toBeInTheDocument();
  });

  it("keeps a continuous polling failure stated instead of letting it time out", async () => {
    vi.useFakeTimers();
    const listRequests = vi
      .fn<RequestsApi["listRequests"]>()
      .mockResolvedValueOnce(requestList)
      .mockRejectedValue(new Error("polling remains unavailable"));
    renderApp({ listRequests });

    await flushEffects();
    await advanceTimers(5000);
    expect(screen.getByRole("alert")).toHaveTextContent("polling remains unavailable");

    // A toast would have auto-dismissed here and left the list unexplained.
    await advanceTimers(8000);
    expect(screen.getByRole("alert")).toHaveTextContent("polling remains unavailable");

    listRequests.mockResolvedValue(requestList);
    await advanceTimers(5000);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
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
