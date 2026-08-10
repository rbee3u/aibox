import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import listStyles from "./components/RecordList.module.css";
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
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
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
    expect(within(banner).getByRole("combobox", { name: "Color theme" })).toHaveValue("system");

    const recordListPanel = screen.getByRole("complementary", { name: "Traffic records" });
    const listHeading = within(recordListPanel).getByRole("heading", {
      name: "Traffic records",
      level: 2,
    });
    expect(listHeading).toBeInTheDocument();
    expect(listHeading.parentElement).toHaveClass(listStyles.listHeader);
    expect(
      within(listHeading.parentElement!).queryByText(String(recordList.total), {
        selector: "span",
      }),
    ).not.toBeInTheDocument();
    const refreshButton = within(recordListPanel).getByRole("button", {
      name: "Refresh traffic records",
    });
    expect(refreshButton).toHaveTextContent("Refresh");
    expect(refreshButton).not.toHaveAttribute("title");
    await waitFor(() =>
      expect(within(recordListPanel).getByRole("button", { name: "Delete all" })).toBeEnabled(),
    );
    expect(within(recordListPanel).getByText("2026-08-06 12:00:01")).toBeInTheDocument();
    const completedRow = within(recordListPanel).getByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    const completedModel = within(completedRow).getByTitle(
      "Model gpt-5.6-sol; Reasoning effort high",
    );
    const completedTiming = within(completedRow).getByTitle("First token 900ms; Duration 1s");
    const completedEnded = within(completedRow).getByTitle("Ended 2026-08-06 12:00:01");
    expect(completedModel).toHaveTextContent("gpt-5.6-sol · high");
    expect(completedTiming).toHaveTextContent("900ms / 1s");
    const completedTarget = within(completedRow).getByTitle(
      "https://api.example.test/v1/responses?stream=true",
    );
    expect(within(completedTarget).getByText("api.example.test")).toHaveClass(
      listStyles.targetHost,
    );
    expect(within(completedTarget).getByText("/v1/responses")).toHaveClass(listStyles.targetPath);
    expect(completedTarget).toHaveTextContent("api.example.test/v1/responses");
    expect(within(completedRow).queryByText(/stream=true/)).not.toBeInTheDocument();
    expect(completedModel.parentElement).toBe(completedTiming.parentElement);
    expect(completedModel.parentElement).toBe(completedEnded.parentElement);
    expect(
      Array.from(completedModel.parentElement!.children)
        .slice(0, 3)
        .map((element) => element.textContent),
    ).toEqual(["gpt-5.6-sol · high", "900ms / 1s", "2026-08-06 12:00:01"]);
    expect(within(completedRow).queryByText("HTTP/2")).not.toBeInTheDocument();
    expect(within(completedRow).getByText("200")).toBeInTheDocument();
    expect(completedRow).toHaveAccessibleDescription(
      "Model gpt-5.6-sol; Reasoning effort high; First token 900ms; Duration 1s; Ended 2026-08-06 12:00:01",
    );
    const activeRow = within(recordListPanel).getByRole("button", {
      name: "GET stream.example.test/events",
    });
    expect(within(activeRow).getByTitle("Ended —")).toHaveTextContent("—");
    expect(within(recordListPanel).getByTitle("First token —; Duration 500ms")).toHaveTextContent(
      "— / 500ms",
    );
  });

  it("applies, persists, and safely resets the color theme", async () => {
    window.localStorage.setItem("aibox-traffic-theme", "dark");
    const user = userEvent.setup();
    render(<App api={fakeApi()} />);

    const theme = screen.getByRole("combobox", { name: "Color theme" });
    expect(theme).toHaveValue("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    await user.selectOptions(theme, "light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(window.localStorage.getItem("aibox-traffic-theme")).toBe("light");

    await user.selectOptions(theme, "system");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(window.localStorage.getItem("aibox-traffic-theme")).toBe("system");
  });

  it("falls back to system appearance and the default split when storage is unavailable", () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("storage unavailable", "SecurityError");
    });
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("storage unavailable", "SecurityError");
    });

    render(<App api={fakeApi()} />);

    expect(screen.getByRole("combobox", { name: "Color theme" })).toHaveValue("system");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(screen.getByRole("separator", { name: "Resize Traffic records panel" })).toHaveAttribute(
      "aria-valuenow",
      "480",
    );
    getItem.mockRestore();
    setItem.mockRestore();
  });

  it("resizes the record panel with pointer and keyboard controls", () => {
    window.localStorage.setItem("aibox-traffic-list-width", "not-a-width");
    render(<App api={fakeApi()} />);

    const splitter = screen.getByRole("separator", { name: "Resize Traffic records panel" });
    expect(splitter).toHaveAttribute("aria-valuenow", "480");

    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter).toHaveAttribute("aria-valuenow", "496");
    expect(window.localStorage.getItem("aibox-traffic-list-width")).toBe("496");

    fireEvent.pointerDown(splitter, { button: 0, pointerId: 1, clientX: 100 });
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 200 });
    fireEvent.pointerUp(splitter, { pointerId: 1, clientX: 200 });
    expect(splitter).toHaveAttribute("aria-valuenow", "596");
    expect(window.localStorage.getItem("aibox-traffic-list-width")).toBe("596");

    fireEvent.keyDown(splitter, { key: "End" });
    expect(splitter).toHaveAttribute("aria-valuenow", "640");
    fireEvent.doubleClick(splitter);
    expect(splitter).toHaveAttribute("aria-valuenow", "480");
  });

  it("prefers effective list metadata and preserves placeholders", async () => {
    const effective = {
      ...completedSummary,
      id: "effective-record",
      incoming_uri: "/https://api.example.test/effective",
      upstream_url: "https://api.example.test/effective",
      protocol: {
        ...completedSummary.protocol!,
        model: { requested: "requested-model", effective: "effective-model" },
        reasoning_effort: { requested: "low", effective: "xhigh" },
      },
    };
    const requestedFallback = {
      ...completedSummary,
      id: "requested-record",
      incoming_uri: "/https://api.example.test/requested",
      upstream_url: "https://api.example.test/requested",
      total_ms: null,
      protocol: {
        ...completedSummary.protocol!,
        model: { requested: null, effective: null },
        reasoning_effort: { requested: "medium", effective: null },
        first_token_at_ns: null,
      },
    };
    const legacy = {
      ...completedSummary,
      id: "legacy-record",
      incoming_uri: "/https://api.example.test/legacy",
      upstream_url: "https://api.example.test/legacy",
      protocol: null,
    };
    render(
      <App
        api={fakeApi({
          listRecords: vi.fn().mockResolvedValue({
            records: [effective, requestedFallback, legacy],
            total: 3,
            deletable_count: 3,
            has_next: false,
          }),
        })}
      />,
    );

    const effectiveRow = await screen.findByRole("button", {
      name: "POST api.example.test/effective",
    });
    expect(
      within(effectiveRow).getByTitle("Model effective-model; Reasoning effort xhigh"),
    ).toHaveTextContent("effective-model · xhigh");

    const requestedRow = screen.getByRole("button", {
      name: "POST api.example.test/requested",
    });
    expect(within(requestedRow).getByTitle("Model —; Reasoning effort medium")).toHaveTextContent(
      "— · medium",
    );
    expect(within(requestedRow).getByTitle("First token —; Duration —")).toHaveTextContent("— / —");

    const legacyRow = screen.getByRole("button", { name: "POST api.example.test/legacy" });
    expect(within(legacyRow).getByTitle("Model —; Reasoning effort —")).toHaveTextContent("— · —");
  });

  it("includes a list issue in the record row's accessible description", async () => {
    const message = "Our servers are currently overloaded. Please try again later.";
    const issueSummary = {
      ...completedSummary,
      assessment: {
        level: "error" as const,
        primary: { source: "provider" as const, kind: "server_error", message },
        issue_count: 1,
      },
    };
    render(
      <App
        api={fakeApi({
          listRecords: vi.fn().mockResolvedValue({
            records: [issueSummary],
            total: 1,
            deletable_count: 1,
            has_next: false,
          }),
        })}
      />,
    );

    const row = await screen.findByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    expect(row).toHaveAccessibleDescription(
      /Record error: Server error\. Our servers are currently overloaded/,
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
    expect(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();
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

    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
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
    const firstPage = { ...recordList, has_next: true };
    const secondPage = {
      records: [completedSummary],
      total: 73,
      deletable_count: 72,
      has_next: false,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
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
    render(<App api={fakeApi({ listRecords })} />);

    await act(async () => Promise.resolve());
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    await act(async () => vi.advanceTimersByTimeAsync(5000));
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: /Next/ }));
    await act(async () => Promise.resolve());
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith(2, expect.any(AbortSignal));
  });

  it("keeps the default browser API stable while navigating pages", async () => {
    const firstPage = { ...recordList, has_next: true };
    const secondPage = {
      records: [completedSummary],
      total: 73,
      deletable_count: 72,
      has_next: false,
    };
    const fetchMock = vi.fn<typeof fetch>().mockImplementation((input) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      const payload = url.includes("?page=2") ? secondPage : firstPage;
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
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/_aibox/traffic/api/records?page=2");
  });

  it("keeps selection controls out of browsing mode and selects records by row", async () => {
    const api = fakeApi();
    const user = userEvent.setup();
    render(<App api={api} />);

    expect(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 · 2 shown · 2 total")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select page" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete selected" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toBeEnabled();
    const activeDelete = screen.getByRole("button", {
      name: "Cannot delete active GET stream.example.test/events",
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
      screen.queryByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete selected" })).toBeDisabled();
    const activeSelection = screen.getByRole("button", {
      name: "Select GET stream.example.test/events",
    });
    expect(activeSelection).toBeDisabled();
    expect(activeSelection.lastElementChild).toHaveAttribute("aria-hidden", "true");
    expect(activeSelection.lastElementChild?.querySelector("svg")).not.toBeInTheDocument();

    const completed = screen.getByRole("button", {
      name: "Select POST api.example.test/v1/responses",
    });
    expect(completed).toHaveAttribute("aria-pressed", "false");
    expect(completed.lastElementChild).toHaveAttribute("aria-hidden", "true");
    expect(completed.lastElementChild?.querySelector("svg")).not.toBeInTheDocument();
    await user.click(completed);

    const selected = screen.getByRole("button", {
      name: "Deselect POST api.example.test/v1/responses",
    });
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh traffic records" })).toHaveFocus(),
    );
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
            has_next: false,
          }),
        })}
      />,
    );

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );

    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select page" }));

    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect POST second.example.test/v1/messages" }),
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
            has_next: false,
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
      has_next: false,
    };
    const afterDelete = {
      records: [secondCompleted],
      total: 1,
      deletable_count: 1,
      has_next: false,
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

    await user.click(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    );
    expect(
      await screen.findByRole("region", { name: "Traffic record details" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deleting POST api.example.test/v1/responses" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Deleting POST api.example.test/v1/responses" }),
    ).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();

    act(() => resolveDelete(1));

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
      ).not.toBeInTheDocument(),
    );
    expect(deleteRecords).toHaveBeenCalledWith([completedSummary.id]);
    expect(screen.getByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a request" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
    ).toHaveFocus();
    expect(screen.queryByText("Record deleted")).not.toBeInTheDocument();
  });

  it("keeps a record when immediate deletion fails", async () => {
    const deleteRecords = vi
      .fn<TrafficApi["deleteRecords"]>()
      .mockRejectedValue(new Error("cannot delete record"));
    const user = userEvent.setup();
    render(<App api={fakeApi({ deleteRecords })} />);

    await user.click(
      await screen.findByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("cannot delete record");
    expect(screen.getByRole("main")).not.toContainElement(alert);
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

  it("keeps an immediately deleted record removed when its list refresh fails", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockRejectedValue(new Error("cannot refresh Traffic Records"));
    const user = userEvent.setup();
    render(
      <App
        api={fakeApi({
          listRecords,
          deleteRecords: vi.fn().mockResolvedValue(1),
        })}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("cannot refresh Traffic Records");
    expect(
      screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
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
      has_next: true,
    };
    const secondPage = {
      records: [secondPageSummary],
      total: 2,
      deletable_count: 2,
      has_next: false,
    };
    const emptySecondPage = {
      records: [],
      total: 1,
      deletable_count: 1,
      has_next: false,
    };
    const firstPageAfterDelete = {
      ...firstPage,
      total: 1,
      deletable_count: 1,
      has_next: false,
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
      await screen.findByRole("button", {
        name: "Delete POST second.example.test/v1/responses",
      }),
    );

    expect(await screen.findByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toHaveFocus();
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
      has_next: true,
    };
    const secondPage = {
      records: [secondPageSummary],
      total: 2,
      deletable_count: 2,
      has_next: false,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords })} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("button", { name: /Next/ }));

    expect(
      await screen.findByRole("button", { name: "Select POST second.example.test/v1/responses" }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Select POST second.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("returns to the first page after deleting a selection across pages", async () => {
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
      has_next: true,
    };
    const secondPage = {
      records: [secondPageSummary],
      total: 2,
      deletable_count: 2,
      has_next: false,
    };
    const afterDelete = {
      records: [],
      total: 0,
      deletable_count: 0,
      has_next: false,
    };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValue(afterDelete);
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords, deleteRecords: vi.fn().mockResolvedValue(2) })} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Select POST second.example.test/v1/responses",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(await screen.findByText("Page 1 · 0 shown · 0 total")).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });

  it("returns to the lowest selected page when selection starts on a later page", async () => {
    const page2Summary = {
      ...completedSummary,
      id: "0198-demo-page-two",
      incoming_uri: "/https://two.example.test/v1/responses",
      upstream_url: "https://two.example.test/v1/responses",
    };
    const page3Summary = {
      ...completedSummary,
      id: "0198-demo-page-three",
      incoming_uri: "/https://three.example.test/v1/responses",
      upstream_url: "https://three.example.test/v1/responses",
    };
    const pages = new Map([
      [1, { records: [completedSummary], total: 3, deletable_count: 3, has_next: true }],
      [2, { records: [page2Summary], total: 3, deletable_count: 3, has_next: true }],
      [3, { records: [page3Summary], total: 3, deletable_count: 3, has_next: false }],
    ]);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockImplementation((page = 1) => Promise.resolve(pages.get(page)!));
    const user = userEvent.setup();
    render(<App api={fakeApi({ listRecords, deleteRecords: vi.fn().mockResolvedValue(2) })} />);

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
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    await waitFor(() => expect(listRecords).toHaveBeenLastCalledWith(2, expect.any(AbortSignal)));
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();
  });

  it("falls back when polling finds the current page empty", async () => {
    vi.useFakeTimers();
    const secondPageSummary = {
      ...completedSummary,
      id: "0198-demo-poll-page",
      incoming_uri: "/https://poll.example.test/v1/responses",
      upstream_url: "https://poll.example.test/v1/responses",
    };
    const firstPage = { ...recordList, has_next: true };
    const secondPage = {
      records: [secondPageSummary],
      total: 51,
      deletable_count: 50,
      has_next: false,
    };
    const emptySecondPage = { records: [], total: 50, deletable_count: 49, has_next: false };
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPage);
    render(<App api={fakeApi({ listRecords })} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await act(async () => Promise.resolve());
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTimeAsync(5000));

    expect(screen.getByText(/^Page 1 ·/)).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });

  it("clears selection on Cancel, keeps focus safe, and ignores Escape", async () => {
    const user = userEvent.setup();
    render(<App api={fakeApi()} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await user.click(cancel);

    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeInTheDocument();
    const select = screen.getByRole("button", { name: "Select" });
    expect(select).toBe(cancel);
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

  it("traps confirmation focus, closes on Escape, and preserves selection", async () => {
    const user = userEvent.setup();
    render(<App api={fakeApi()} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected record?" });
    const cancelDialog = within(dialog).getByRole("button", { name: "Cancel" });
    const confirm = within(dialog).getByRole("button", { name: "Delete permanently" });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(cancelDialog).toHaveFocus();
    fireEvent.keyDown(cancelDialog, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(dialog).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Delete selected" })).toHaveFocus(),
    );
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("keeps selection mode and selected ids when deletion fails", async () => {
    const api = fakeApi({ deleteRecords: vi.fn().mockRejectedValue(new Error("delete failed")) });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(await screen.findByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("button", { name: "Delete selected" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("delete failed");
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected record?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect POST api.example.test/v1/responses" }),
    ).toHaveAttribute("aria-pressed", "true");
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
          bytes: encoder.encode(kind === "request" ? "request body" : "data: response body\n\n"),
          nextOffset: kind === "request" ? 12 : 21,
        }),
      ),
    });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    );
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getByRole("tab", { name: "Summary" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(detail).getByText("First token")).toBeInTheDocument();
    expect(within(detail).getByText("gpt-5.6-sol")).toBeInTheDocument();
    expect(within(detail).getByText("Streaming")).toBeInTheDocument();
    expect(within(detail).getByRole("list", { name: "Timing stages" })).toBeInTheDocument();
    expect(
      within(detail).getByRole("listitem", { name: "Proxy setup: 100 ms" }),
    ).toBeInTheDocument();
    expect(within(detail).getByRole("heading", { name: "Token usage" })).toBeInTheDocument();
    expect(within(detail).getByText("12,000")).toBeInTheDocument();
    expect(within(detail).getByText("No diagnostics.")).toBeInTheDocument();
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
    await user.click(await screen.findByRole("button", { name: /message/ }));
    expect(screen.getByText("response body")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select" }));
    await user.click(
      screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
    );
    expect(screen.getByText("response body")).toBeInTheDocument();
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const getRecordMock = api.getRecord as unknown as { mock: { calls: unknown[][] } };
    expect(getRecordMock.mock.calls).toHaveLength(1);
    expect(getRecordMock.mock.calls[0]).toEqual([completedSummary.id, expect.any(AbortSignal)]);
  });

  it("loads zstd decoded Source only after the complete raw Body is available", async () => {
    const encoded = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);
    const decoded = new TextEncoder().encode('{"model":"gpt-5.6-sol"}');
    const detail = {
      ...completedDetail,
      request: {
        ...completedDetail.request,
        headers: [
          ...completedDetail.request.headers,
          { name: "Content-Encoding", value_base64: btoa(" ZsTd ") },
        ],
      },
      request_body_bytes: encoded.length,
    };
    const loadDecodedBody = vi.fn<TrafficApi["loadDecodedBody"]>().mockResolvedValue(decoded);
    const api = fakeApi({
      getRecord: vi.fn().mockResolvedValue(detail),
      loadBody: vi.fn().mockResolvedValue({ bytes: encoded, nextOffset: encoded.length }),
      loadDecodedBody,
    });
    const user = userEvent.setup();
    render(<App api={api} />);

    await user.click(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    );
    await user.click(screen.getByRole("tab", { name: "Request" }));

    await waitFor(() => expect(loadDecodedBody).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("tabpanel").textContent).toContain('"model"'));
    expect(screen.getByText('"gpt-5.6-sol"')).toBeInTheDocument();
    expect(loadDecodedBody).toHaveBeenCalledWith(
      completedSummary.id,
      "request",
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("button", { name: "Pretty" })).toHaveAttribute("aria-pressed", "true");
  });

  it("does not restart a slow zstd decode for unchanged active detail polls", async () => {
    vi.useFakeTimers();
    const encoded = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);
    const decoded = new TextEncoder().encode('{"state":"ready"}');
    const zstdDetail = {
      ...activeDetail,
      request: {
        ...activeDetail.request,
        headers: [
          ...activeDetail.request.headers,
          { name: "content-encoding", value_base64: btoa("zstd") },
        ],
      },
      request_body_bytes: encoded.length,
    };
    const getRecord = vi.fn<TrafficApi["getRecord"]>().mockImplementation(() =>
      Promise.resolve({
        ...zstdDetail,
        request: { ...zstdDetail.request },
        summary: { ...zstdDetail.summary },
      }),
    );
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, _kind, offset) =>
      Promise.resolve({
        bytes: offset === 0 ? encoded : new Uint8Array(),
        nextOffset: encoded.length,
      }),
    );
    let decodedSignal: AbortSignal | undefined;
    let resolveDecoded!: (value: Uint8Array) => void;
    const decodedRequest = new Promise<Uint8Array>((resolve) => (resolveDecoded = resolve));
    const loadDecodedBody = vi
      .fn<TrafficApi["loadDecodedBody"]>()
      .mockImplementation((_id, _kind, signal) => {
        decodedSignal = signal;
        return decodedRequest;
      });
    render(
      <App
        api={fakeApi({
          listRecords: vi.fn().mockResolvedValue({
            records: [activeSummary],
            total: 1,
            deletable_count: 0,
            has_next: false,
          }),
          getRecord,
          loadBody,
          loadDecodedBody,
        })}
      />,
    );

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("tab", { name: "Request" }));
    await act(async () => Promise.resolve());
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);

    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(loadBody).toHaveBeenCalledTimes(1);
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);
    expect(decodedSignal?.aborted).toBe(false);

    await act(async () => {
      resolveDecoded(decoded);
      await decodedRequest;
    });
    expect(screen.getByText('"ready"')).toBeInTheDocument();
  });

  it("clears a selected record even when its pending detail request ignores abort", async () => {
    const pendingDetail = new Promise<typeof completedDetail>(() => undefined);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockResolvedValue({
        records: [activeSummary],
        total: 1,
        deletable_count: 0,
        has_next: false,
      });
    const user = userEvent.setup();
    render(
      <App
        api={fakeApi({
          listRecords,
          getRecord: vi.fn().mockReturnValue(pendingDetail),
          deleteAll: vi.fn().mockResolvedValue(1),
        })}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    );
    expect(screen.getByText("Loading record…")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete all" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    expect(await screen.findByRole("heading", { name: "Select a request" })).toBeInTheDocument();
    expect(screen.queryByText("Loading record…")).not.toBeInTheDocument();
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

    await user.click(await screen.findByRole("button", { name: "GET stream.example.test/events" }));
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

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("cannot scan Traffic Records");
    const listPanel = screen.getByRole("complementary", { name: "Traffic records" });
    expect(listPanel.parentElement).toContainElement(alert);
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();
  });

  it("keeps simultaneous list and detail failures in their own regions", async () => {
    vi.useFakeTimers();
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockRejectedValue(new Error("list polling failed"));
    render(
      <App
        api={fakeApi({
          listRecords,
          getRecord: vi.fn().mockRejectedValue(new Error("detail loading failed")),
        })}
      />,
    );

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "POST api.example.test/v1/responses" }));
    await act(async () => Promise.resolve());
    expect(screen.getByRole("alert")).toHaveTextContent("detail loading failed");

    await act(async () => vi.advanceTimersByTimeAsync(5000));
    const alerts = screen.getAllByRole("alert");
    expect(alerts).toHaveLength(2);
    expect(alerts.some((alert) => alert.textContent?.includes("list polling failed"))).toBe(true);
    expect(alerts.some((alert) => alert.textContent?.includes("detail loading failed"))).toBe(true);
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
    expect(
      screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GET stream.example.test/events" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
  });

  it("ignores an older list response after a refresh", async () => {
    let resolveInitial!: (value: typeof recordList) => void;
    const initial = new Promise<typeof recordList>((resolve) => (resolveInitial = resolve));
    const refreshed = {
      records: [completedSummary],
      total: 1,
      deletable_count: 1,
      has_next: false,
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
    expect(
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
    ).toBeInTheDocument();

    await act(async () => {
      resolveInitial(recordList);
      await initial;
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
        has_next: false,
      }),
      getRecord,
    });
    render(<App api={api} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
    await act(async () => Promise.resolve());
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getAllByText("Waiting").length).toBeGreaterThan(0);

    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(within(detail).queryAllByText("Waiting")).toHaveLength(0);
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
    const getRecord = vi.fn<TrafficApi["getRecord"]>().mockResolvedValue({
      ...activeDetail,
      summary: {
        ...activeDetail.summary,
        timing: {
          ...activeDetail.summary.timing,
          upstream_request_body_completed_at_ns: null,
        },
      },
    });
    const api = fakeApi({
      listRecords: vi.fn().mockResolvedValue({
        records: [activeSummary],
        total: 1,
        deletable_count: 0,
        has_next: false,
      }),
      getRecord,
      loadBody,
    });
    render(<App api={api} />);

    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
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
