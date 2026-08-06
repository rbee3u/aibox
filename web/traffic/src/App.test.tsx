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

    expect(await screen.findByText("aibox traffic")).toBeInTheDocument();
    expect(screen.getByText("Understand your model API traffic")).toBeInTheDocument();
    expect(screen.queryByText("temporary HTTP/SSE inspector")).not.toBeInTheDocument();
    expect(container.querySelector("header svg")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByText("ai")).not.toBeInTheDocument();

    const banner = screen.getByRole("banner");
    const resources = within(banner).getByRole("navigation", { name: "Resources" });
    const links = [
      ["GitHub", "https://github.com/rbee3u/aibox"],
      ["Codex docs", "https://developers.openai.com/codex/cli"],
      ["Claude docs", "https://code.claude.com/docs/en/overview"],
    ] as const;
    for (const [name, href] of links) {
      const link = within(resources).getByRole("link", { name });
      expect(link).toHaveAttribute("href", href);
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noopener noreferrer");
    }
    expect(within(banner).queryByRole("button")).not.toBeInTheDocument();

    const recordListPanel = screen.getByRole("complementary", { name: "Traffic records" });
    expect(
      within(recordListPanel).getByRole("button", { name: "Refresh traffic records" }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(within(recordListPanel).getByRole("button", { name: "Delete all" })).toBeEnabled(),
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

    await act(async () => vi.advanceTimersByTimeAsync(2500));
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: /Next/ }));
    await act(async () => Promise.resolve());
    expect(screen.getByText("Page 2")).toBeInTheDocument();
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

    expect(await screen.findByText("Page 2")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/_aibox/traffic/api/records?cursor=next-page");
  });

  it("renders records and keeps active records out of deletion selection", async () => {
    const api = fakeApi();
    const user = userEvent.setup();
    render(<App api={api} />);

    expect(await screen.findByText("api.example.test")).toBeInTheDocument();
    expect(screen.getByText("stream.example.test")).toBeInTheDocument();
    expect(screen.getAllByText("2 records")).toHaveLength(1);
    expect(
      screen.getByRole("checkbox", { name: /Select GET stream\.example\.test/ }),
    ).toBeDisabled();
    expect(screen.getByText("Page 1")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: /Select POST api\.example\.test/ }));
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.queryByText("Page 1")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Delete selected/ }));
    expect(screen.getByRole("dialog", { name: "Delete 1 selected record?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    // eslint-disable-next-line @typescript-eslint/unbound-method
    const deleteRecordsMock = api.deleteRecords as unknown as { mock: { calls: unknown[][] } };
    await waitFor(() =>
      expect(deleteRecordsMock.mock.calls).toContainEqual([[completedSummary.id]]),
    );
  });

  it("shows a mixed Select page state for a partial page selection", async () => {
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

    await user.click(
      await screen.findByRole("checkbox", { name: /Select POST api\.example\.test/ }),
    );

    expect(screen.getByRole("checkbox", { name: "Select page" })).toBePartiallyChecked();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("disables page and global deletion when no completed records exist", async () => {
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

    expect(
      await screen.findByRole("checkbox", { name: /Select GET stream\.example\.test/ }),
    ).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "Select page" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
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

    await user.click(
      await screen.findByRole("checkbox", { name: /Select POST api\.example\.test/ }),
    );
    await user.click(screen.getByRole("button", { name: /Next/ }));

    expect(await screen.findByText("second.example.test")).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Select page" })).not.toBeChecked();
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

    await user.click(await screen.findByRole("button", { name: /POST api\.example\.test/ }));
    expect(await screen.findByText("request body")).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Response" }));
    expect(screen.getByText("response body")).toBeInTheDocument();
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const getRecordMock = api.getRecord as unknown as { mock: { calls: unknown[][] } };
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

    await user.click(await screen.findByRole("button", { name: /GET stream\.example\.test/ }));
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
    fireEvent.click(screen.getByRole("button", { name: /GET stream\.example\.test/ }));
    await act(async () => Promise.resolve());
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getByText("Active")).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(within(detail).queryByText("Active")).not.toBeInTheDocument();
  });

  it("does not overlap active body polls", async () => {
    vi.useFakeTimers();
    const encoder = new TextEncoder();
    let requestPoll!: (value: { bytes: Uint8Array; nextOffset: number }) => void;
    let responsePoll!: (value: { bytes: Uint8Array; nextOffset: number }) => void;
    const requestPollResult = new Promise<{ bytes: Uint8Array; nextOffset: number }>(
      (resolve) => (requestPoll = resolve),
    );
    const responsePollResult = new Promise<{ bytes: Uint8Array; nextOffset: number }>(
      (resolve) => (responsePoll = resolve),
    );
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, kind, offset) => {
      if (offset === 0) return Promise.resolve({ bytes: encoder.encode(kind), nextOffset: 1 });
      return kind === "request" ? requestPollResult : responsePollResult;
    });
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
      loadBody,
    });
    render(<App api={api} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: /GET stream\.example\.test/ }));
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(loadBody).toHaveBeenCalledTimes(4);

    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(loadBody).toHaveBeenCalledTimes(4);

    await act(async () => {
      requestPoll({ bytes: encoder.encode("next"), nextOffset: 5 });
      responsePoll({ bytes: encoder.encode("next"), nextOffset: 5 });
      await Promise.resolve();
    });
    expect(getRecord).toHaveBeenCalledTimes(2);
  });
});
