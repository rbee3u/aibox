import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  activeDetail,
  activeSummary,
  completedDetail,
  completedSummary,
  fakeApi,
  recordList,
} from "./test/fixtures";
import type { TrafficApi } from "./types";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("Traffic App", () => {
  it("keeps management actions by the list and exposes lightweight resource links", async () => {
    const api = fakeApi();
    const { container } = render(<App api={api} />);

    const brandName = await screen.findByText("AIBox Traffic");
    const tagline = screen.getByText("Inspect your LLM requests");
    expect(brandName.parentElement).toBe(tagline.parentElement);
    expect(screen.queryByText("temporary HTTP/SSE inspector")).not.toBeInTheDocument();
    expect(container.querySelector("header svg")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByText("ai")).not.toBeInTheDocument();

    const banner = screen.getByRole("banner");
    const resources = within(banner).getByRole("navigation", { name: "Resources" });
    const links = [
      ["Codex docs", "https://developers.openai.com/codex/cli"],
      ["Claude docs", "https://code.claude.com/docs/en/overview"],
      ["GitHub", "https://github.com/rbee3u/aibox"],
    ] as const;
    expect(
      within(resources)
        .getAllByRole("link")
        .map((link) => link.textContent?.trim()),
    ).toEqual(links.map(([name]) => name));
    for (const [name, href] of links) {
      const link = within(resources).getByRole("link", { name });
      expect(link).toHaveAttribute("href", href);
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noopener noreferrer");
    }
    expect(within(banner).queryByRole("button")).not.toBeInTheDocument();

    const recordListPanel = screen.getByRole("complementary", { name: "Traffic records" });
    expect(
      within(recordListPanel).getByRole("heading", { name: "Traffic records", level: 2 }),
    ).toBeInTheDocument();
    const refreshButton = within(recordListPanel).getByRole("button", {
      name: "Refresh traffic records",
    });
    expect(refreshButton).toHaveTextContent("Refresh");
    expect(refreshButton).not.toHaveAttribute("title");
    await waitFor(() =>
      expect(within(recordListPanel).getByRole("button", { name: "Delete all" })).toBeEnabled(),
    );
    expect(within(recordListPanel).getByText("2026-08-06 12:00:00")).toBeInTheDocument();
    expect(within(recordListPanel).getByText("2026-08-06 12:01:00")).toBeInTheDocument();
    const completedTiming = within(recordListPanel).getByTitle("First token —; Duration 1s");
    expect(completedTiming).toHaveTextContent("— / 1s");
    const completedRow = within(recordListPanel).getByRole("button", {
      name: "POST api.example.test",
    });
    expect(within(completedRow).getByText("HTTP/2")).toBeInTheDocument();
    expect(within(completedRow).getByText("200")).toBeInTheDocument();
    expect(completedRow).toHaveAccessibleDescription("First token —; Duration 1s");
    expect(within(recordListPanel).getByTitle("First token —; Duration 500ms")).toHaveTextContent(
      "— / 500ms",
    );
  });

  it("keeps Refresh enabled while a background list load is pending", async () => {
    let resolveList!: (value: typeof recordList) => void;
    const api = fakeApi({
      listRecords: vi.fn().mockReturnValue(
        new Promise<typeof recordList>((resolve) => {
          resolveList = resolve;
        }),
      ),
    });
    render(<App api={api} />);

    expect(screen.getByRole("button", { name: /Refresh/ })).toBeEnabled();
    resolveList(recordList);
    expect(await screen.findByText("api.example.test")).toBeInTheDocument();
  });

  it("marks the list refresh busy until a manual refresh completes", async () => {
    let resolveRefresh!: (value: typeof recordList) => void;
    const refresh = new Promise<typeof recordList>((resolve) => {
      resolveRefresh = resolve;
    });
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockReturnValueOnce(refresh);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords })} />);

    await screen.findByText("api.example.test");
    const refreshButton = screen.getByRole("button", {
      name: "Refresh traffic records",
    });
    await user.click(refreshButton);

    expect(screen.getByRole("button", { name: "Refreshing traffic records" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refreshing traffic records" })).toHaveAttribute(
      "aria-busy",
      "true",
    );

    resolveRefresh(recordList);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeEnabled(),
    );
  });

  it("keeps Next clickable while a background list refresh is pending", async () => {
    vi.useFakeTimers();
    const firstPage = { ...recordList, next_cursor: "next-page" };
    const secondPage = {
      records: [completedSummary],
      total: 73,
      deletable_count: 72,
      next_cursor: null,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockImplementationOnce(
        (_cursor, signal) =>
          new Promise((_resolve, reject) => {
            signal?.addEventListener("abort", () =>
              reject(new DOMException("Aborted", "AbortError")),
            );
          }),
      )
      .mockResolvedValueOnce(secondPage);
    render(<App api={fakeApi({ listRecords })} />);

    await act(async () => Promise.resolve());
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    await act(async () => vi.advanceTimersByTimeAsync(5000));
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: /Next/ }));
    await act(async () => Promise.resolve());
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith("next-page", expect.any(AbortSignal));
  });

  it("keeps the default browser API stable while navigating pages", async () => {
    const firstPage = { ...recordList, next_cursor: "next-page" };
    const secondPage = {
      records: [completedSummary],
      total: 73,
      deletable_count: 72,
      next_cursor: null,
    };
    const fetchMock = vi.fn<typeof fetch>().mockImplementation((input) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      const payload = url.includes("?cursor=") ? secondPage : firstPage;
      return Promise.resolve(
        new Response(JSON.stringify(payload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      );
    });
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /Next/ }));

    expect(await screen.findByText(/^Page 2 ·/)).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/_aibox/traffic/api/records?cursor=next-page");
  });

  it("keeps selection controls out of browsing mode and selects records by row", async () => {
    const api = fakeApi();
    const user = userEvent.setup();
    render(<App api={api} />);

    expect(await screen.findByText("api.example.test")).toBeInTheDocument();
    expect(screen.getByText("stream.example.test")).toBeInTheDocument();
    expect(screen.getByText("Page 1 · 2 shown · 2 total")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select page" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete POST api.example.test" })).toBeEnabled();
    const activeDelete = screen.getByRole("button", {
      name: "Cannot delete active GET stream.example.test",
    });
    expect(activeDelete).toBeDisabled();
    expect(activeDelete.parentElement).toHaveAttribute("title", "Active records cannot be deleted");
    const normalActions = screen.getByRole("button", {
      name: "Refresh traffic records",
    }).parentElement;
    expect(
      within(normalActions!)
        .getAllByRole("button")
        .map((button) => button.textContent?.trim()),
    ).toEqual(["Refresh", "Delete all", "Select"]);

    await user.click(screen.getByRole("button", { name: "Select" }));
    const selectPage = screen.getByRole("button", { name: "Select page" });
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(selectPage.parentElement).toContainElement(cancel);
    expect(cancel.parentElement?.firstElementChild).toHaveTextContent("0 selected");
    expect(
      within(cancel.parentElement!)
        .getAllByRole("button")
        .map((button) => button.textContent?.trim()),
    ).toEqual(["Delete selected", "Cancel"]);
    expect(screen.queryByRole("heading", { name: "Traffic records" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refresh traffic records" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Delete POST api.example.test" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete selected" })).toBeDisabled();
    const activeSelection = screen.getByRole("button", { name: "Select GET stream.example.test" });
    expect(activeSelection).toBeDisabled();
    expect(activeSelection.lastElementChild).toHaveAttribute("aria-hidden", "true");
    expect(activeSelection.lastElementChild?.querySelector("svg")).not.toBeInTheDocument();

    const completed = screen.getByRole("button", { name: "Select POST api.example.test" });
    expect(completed).toHaveAttribute("aria-pressed", "false");
    expect(completed.lastElementChild).toHaveAttribute("aria-hidden", "true");
    expect(completed.lastElementChild?.querySelector("svg")).not.toBeInTheDocument();
    await user.click(completed);

    const selected = screen.getByRole("button", { name: "Deselect POST api.example.test" });
    expect(selected).toHaveAttribute("aria-pressed", "true");
    expect(selected.lastElementChild).toHaveAttribute("aria-hidden", "true");
    expect(selected.lastElementChild?.querySelector("svg")).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Delete selected/ }));
    expect(screen.getByRole("dialog", { name: "Delete 1 selected record?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    // eslint-disable-next-line @typescript-eslint/unbound-method
    const deleteRecordsMock = api.deleteRecords as unknown as { mock: { calls: unknown[][] } };
    await waitFor(() =>
      expect(deleteRecordsMock.mock.calls).toContainEqual([[completedSummary.id]]),
    );
    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
  });

  it("selects, completes, and clears the current page without checkboxes", async () => {
    const secondCompleted = {
      ...completedSummary,
      id: "0198-demo-completed-second",
      incoming_uri: "/https://second.example.test/v1/messages",
      upstream_url: "https://second.example.test/v1/messages",
    };
    const user = userEvent.setup();
    render(
      <App
        api={fakeApi({
          listRecords: vi.fn().mockResolvedValue({
            records: [activeSummary, completedSummary, secondCompleted],
            total: 3,
            deletable_count: 2,
            next_cursor: null,
          }),
        })}
      />,
    );

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));

    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select page" }));

    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect POST second.example.test" }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Clear page" }));

    expect(screen.getByText("0 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete selected" })).toBeDisabled();
  });

  it("disables selection and global deletion when no completed records exist", async () => {
    render(
      <App
        api={fakeApi({
          listRecords: vi.fn().mockResolvedValue({
            records: [activeSummary],
            total: 1,
            deletable_count: 0,
            next_cursor: null,
          }),
        })}
      />,
    );

    expect(await screen.findByRole("button", { name: "Select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select page" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
  });

  it("deletes one record without confirmation, locks deletion, clears detail, and restores focus", async () => {
    const secondCompleted = {
      ...completedSummary,
      id: "0198-demo-completed-second",
      incoming_uri: "/https://second.example.test/v1/responses",
      upstream_url: "https://second.example.test/v1/responses",
    };
    const initial = {
      records: [completedSummary, secondCompleted],
      total: 2,
      deletable_count: 2,
      next_cursor: null,
    };
    const afterDelete = {
      records: [secondCompleted],
      total: 1,
      deletable_count: 1,
      next_cursor: null,
    };
    let resolveDelete!: (deleted: number) => void;
    const deleteRequest = new Promise<number>((resolve) => (resolveDelete = resolve));
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(initial)
      .mockResolvedValue(afterDelete);
    const deleteRecords = vi.fn<TrafficApi["deleteRecords"]>().mockReturnValue(deleteRequest);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords, deleteRecords })} />);

    await user.click(await screen.findByRole("button", { name: "POST api.example.test" }));
    expect(
      await screen.findByRole("region", { name: "Traffic record details" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete POST api.example.test" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deleting POST api.example.test" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deleting POST api.example.test" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(screen.getByRole("button", { name: "Delete POST second.example.test" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();

    act(() => resolveDelete(1));

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "POST api.example.test" }),
      ).not.toBeInTheDocument(),
    );
    expect(deleteRecords).toHaveBeenCalledWith([completedSummary.id]);
    expect(screen.getByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a request" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete POST second.example.test" })).toHaveFocus();
    expect(screen.queryByText("Record deleted")).not.toBeInTheDocument();
  });

  it("keeps a record when immediate deletion fails", async () => {
    const deleteRecords = vi
      .fn<TrafficApi["deleteRecords"]>()
      .mockRejectedValue(new Error("cannot delete record"));
    const user = userEvent.setup();
    render(<App api={fakeApi({ deleteRecords })} />);

    await user.click(await screen.findByRole("button", { name: "Delete POST api.example.test" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot delete record");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "POST api.example.test" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete POST api.example.test" })).toBeEnabled();
  });

  it("returns to the previous page when immediate deletion empties the current page", async () => {
    const secondPageSummary = {
      ...completedSummary,
      id: "0198-demo-second-page",
      incoming_uri: "/https://second.example.test/v1/responses",
      upstream_url: "https://second.example.test/v1/responses",
    };
    const firstPage = {
      records: [completedSummary],
      total: 2,
      deletable_count: 2,
      next_cursor: "second-page",
    };
    const secondPage = {
      records: [secondPageSummary],
      total: 2,
      deletable_count: 2,
      next_cursor: null,
    };
    const emptySecondPage = {
      records: [],
      total: 1,
      deletable_count: 1,
      next_cursor: null,
    };
    const firstPageAfterDelete = {
      ...firstPage,
      total: 1,
      deletable_count: 1,
      next_cursor: null,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPageAfterDelete);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords, deleteRecords: vi.fn().mockResolvedValue(1) })} />);

    await user.click(await screen.findByRole("button", { name: "Next" }));
    await user.click(
      await screen.findByRole("button", { name: "Delete POST second.example.test" }),
    );

    expect(await screen.findByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete POST api.example.test" })).toHaveFocus();
  });

  it("preserves the selected count across record pages", async () => {
    const secondPageSummary = {
      ...completedSummary,
      id: "0198-demo-second-page",
      incoming_uri: "/https://second.example.test/v1/responses",
      upstream_url: "https://second.example.test/v1/responses",
    };
    const firstPage = {
      records: [completedSummary],
      total: 2,
      deletable_count: 2,
      next_cursor: "second-page",
    };
    const secondPage = {
      records: [secondPageSummary],
      total: 2,
      deletable_count: 2,
      next_cursor: null,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords })} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    await user.click(screen.getByRole("button", { name: /Next/ }));

    expect(await screen.findByText("second.example.test")).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select POST second.example.test" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("clears selection on Cancel, keeps focus safe, and ignores Escape", async () => {
    const user = userEvent.setup();
    render(<App api={fakeApi()} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await user.click(cancel);

    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeInTheDocument();
    const select = screen.getByRole("button", { name: "Select" });
    expect(select).toBe(cancel);
    expect(select).toHaveFocus();
    await user.click(select);
    expect(screen.getByText("0 selected")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("ignores Escape in confirmation dialogs and selection mode", async () => {
    const user = userEvent.setup();
    render(<App api={fakeApi()} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected record?" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(dialog).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("keeps selection mode and selected ids when deletion fails", async () => {
    const api = fakeApi({ deleteRecords: vi.fn().mockRejectedValue(new Error("delete failed")) });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("delete failed");
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected record?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect POST api.example.test" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("pauses list polling during selection and refreshes immediately after exit", async () => {
    vi.useFakeTimers();
    const listRecords = vi.fn<TrafficApi["listRecords"]>().mockResolvedValue(recordList);
    render(<App api={fakeApi({ listRecords })} />);

    await act(async () => Promise.resolve());
    expect(screen.getByRole("button", { name: "Select" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    await act(async () => vi.advanceTimersByTimeAsync(7500));

    expect(listRecords).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "Refresh traffic records" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => Promise.resolve());
    expect(listRecords).toHaveBeenCalledTimes(2);
  });

  it("loads request and response bodies when a record is selected", async () => {
    const encoder = new TextEncoder();
    const api = fakeApi({
      loadBody: vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, kind) =>
        Promise.resolve({
          bytes: encoder.encode(kind === "request" ? "request body" : "response body"),
          nextOffset: kind === "request" ? 12 : 13,
        }),
      ),
    });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(await screen.findByRole("button", { name: "POST api.example.test" }));
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getByRole("tab", { name: "Summary" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(detail).getByText("First token")).toBeInTheDocument();
    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(api.loadBody).not.toHaveBeenCalled();
    await user.click(within(detail).getByRole("tab", { name: "Request" }));
    expect(await screen.findByText("request body")).toBeInTheDocument();
    const requestLine = within(detail).getByText("POST").parentElement;
    expect(requestLine).toContainElement(within(detail).getByText("POST"));
    expect(requestLine).toContainElement(within(detail).getByText("https://api.example.test"));
    expect(requestLine).toContainElement(within(detail).getByText("/v1/responses?stream=true"));
    expect(within(detail).getByLabelText("HTTP/2 200 OK")).toBeInTheDocument();
    expect(within(detail).queryByText("Query parameters")).not.toBeInTheDocument();
    await user.click(within(detail).getByRole("tab", { name: "Response" }));
    expect(screen.getByText("response body")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select" }));
    await user.click(screen.getByRole("button", { name: "Select POST api.example.test" }));
    expect(screen.getByText("response body")).toBeInTheDocument();
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const getRecordMock = api.getRecord as unknown as { mock: { calls: unknown[][] } };
    expect(getRecordMock.mock.calls).toHaveLength(1);
    expect(getRecordMock.mock.calls[0]).toEqual([completedSummary.id, expect.any(AbortSignal)]);
  });

  it("keeps an active selection while its detail is still loading during delete-all", async () => {
    let resolveDetail!: (value: typeof activeDetail) => void;
    const detailRequest = new Promise<typeof activeDetail>((resolve) => (resolveDetail = resolve));
    const api = fakeApi({
      getRecord: vi.fn().mockReturnValue(detailRequest),
      deleteAll: vi.fn().mockResolvedValue(1),
    });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(await screen.findByRole("button", { name: "GET stream.example.test" }));
    await user.click(screen.getByRole("button", { name: "Delete all" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    // eslint-disable-next-line @typescript-eslint/unbound-method
    const deleteAllMock = api.deleteAll as unknown as { mock: { calls: unknown[][] } };
    await waitFor(() => expect(deleteAllMock.mock.calls).toContainEqual([1]));
    expect(screen.getByText("Loading record…")).toBeInTheDocument();
    resolveDetail(activeDetail);
  });

  it("shows list failures and retries in place", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockRejectedValueOnce(new Error("cannot scan Traffic Records"))
      .mockResolvedValue(recordList);
    const api = fakeApi({ listRecords });
    const user = userEvent.setup();
    render(<App api={api} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot scan Traffic Records");
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("api.example.test")).toBeInTheDocument();
  });

  it("keeps a post-delete list refresh failure visible", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockRejectedValue(new Error("cannot refresh Traffic Records"));
    const api = fakeApi({ listRecords, deleteAll: vi.fn().mockResolvedValue(1) });
    const user = userEvent.setup();
    render(<App api={api} />);

    const deleteAll = await screen.findByRole("button", { name: "Delete all" });
    await waitFor(() => expect(deleteAll).toBeEnabled());
    await user.click(deleteAll);
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot refresh Traffic Records");
  });

  it("ignores an older list response after a refresh", async () => {
    let resolveInitial!: (value: typeof recordList) => void;
    const initial = new Promise<typeof recordList>((resolve) => (resolveInitial = resolve));
    const refreshed = {
      records: [completedSummary],
      total: 1,
      deletable_count: 1,
      next_cursor: null,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockReturnValueOnce(initial)
      .mockResolvedValueOnce(refreshed);
    const api = fakeApi({ listRecords });
    const replacementApi = fakeApi({ listRecords: vi.fn().mockResolvedValue(refreshed) });
    const { rerender } = render(<App api={api} />);

    await act(async () => Promise.resolve());
    rerender(<App api={replacementApi} />);
    expect(await screen.findByText("api.example.test")).toBeInTheDocument();

    await act(async () => {
      resolveInitial(recordList);
      await initial;
    });
    expect(screen.getByText("api.example.test")).toBeInTheDocument();
    expect(screen.queryByText("stream.example.test")).not.toBeInTheDocument();
  });

  it("polls an active detail until it completes", async () => {
    vi.useFakeTimers();
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockResolvedValueOnce({
        ...completedDetail,
        request: { ...completedDetail.request, id: activeSummary.id },
      });
    const api = fakeApi({
      listRecords: vi.fn().mockResolvedValue({
        records: [activeSummary],
        total: 1,
        deletable_count: 0,
        next_cursor: null,
      }),
      getRecord,
    });
    render(<App api={api} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test" }));
    await act(async () => Promise.resolve());
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getByText("Waiting")).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(within(detail).queryByText("Waiting")).not.toBeInTheDocument();
    expect(within(detail).getByLabelText("HTTP/2 200 OK")).toBeInTheDocument();
  });

  it("does not overlap active body polls", async () => {
    vi.useFakeTimers();
    const encoder = new TextEncoder();
    let requestPoll!: (value: { bytes: Uint8Array; nextOffset: number }) => void;
    const requestPollResult = new Promise<{ bytes: Uint8Array; nextOffset: number }>(
      (resolve) => (requestPoll = resolve),
    );
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, kind, offset) => {
      if (offset === 0) return Promise.resolve({ bytes: encoder.encode(kind), nextOffset: 1 });
      return requestPollResult;
    });
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockResolvedValue(activeDetail);
    const api = fakeApi({
      listRecords: vi.fn().mockResolvedValue({
        records: [activeSummary],
        total: 1,
        deletable_count: 0,
        next_cursor: null,
      }),
      getRecord,
      loadBody,
    });
    render(<App api={api} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test" }));
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("tab", { name: "Request" }));
    await act(async () => Promise.resolve());
    expect(loadBody).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(loadBody).toHaveBeenCalledTimes(2);

    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(loadBody).toHaveBeenCalledTimes(2);

    await act(async () => {
      requestPoll({ bytes: encoder.encode("next"), nextOffset: 5 });
      await Promise.resolve();
    });
    expect(loadBody).toHaveBeenCalledTimes(2);
  });
});
