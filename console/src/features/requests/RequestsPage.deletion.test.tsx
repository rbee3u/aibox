import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import {
  activeDetail,
  activeRequestList,
  activeSummary,
  completedDetail,
  completedSummary,
  completedSummaryFor,
  requestList,
  requestListFor,
} from "@/features/requests/testFixtures";
import {
  advanceTimers,
  confirmDeletion,
  confirmSingleDeletion,
  flushEffects,
  openActiveRecord,
  openCompletedRecord,
  renderApp,
  selectCompletedRecord,
} from "@/features/requests/testHarness";
import { deferred } from "@/test/deferred";

describe("Requests page selection and deletion", () => {
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
    await user.click(screen.getByRole("button", { name: "Select Requests" }));
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
    expect(screen.queryByRole("button", { name: "Refresh Requests" })).not.toBeInTheDocument();
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

    await user.click(await screen.findByRole("button", { name: "Select Requests" }));
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

    expect(await screen.findByRole("button", { name: "Select Requests" })).toBeDisabled();
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
    expect(screen.getByRole("button", { name: "Select Requests" })).toBeDisabled();

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

    expect(screen.getByRole("button", { name: "Refresh Requests" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    await advanceTimers(10_000);
    expect(listRequests).toHaveBeenCalledTimes(1);

    await act(async () => {
      deleteRequest.resolve(1);
      await deleteRequest.promise;
    });
    expect(listRequests.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByRole("button", { name: "Refresh Requests" })).toBeEnabled();
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
    expect(screen.getByRole("button", { name: "Refresh Requests" })).toHaveFocus();
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
    await user.click(screen.getByRole("button", { name: "Select Requests" }));
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

  it("clears selection on Cancel, keeps focus safe, and ignores Escape", async () => {
    const user = userEvent.setup();
    renderApp();

    await selectCompletedRecord(user);
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await user.click(cancel);

    expect(screen.getByRole("button", { name: "Refresh Requests" })).toBeInTheDocument();
    const select = screen.getByRole("button", { name: "Select Requests" });
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
    expect(screen.getByRole("button", { name: "Select Requests" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Select Requests" }));
    await advanceTimers(7500);

    expect(listRequests).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Refresh Requests" })).not.toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "Select Requests" }));
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
});
