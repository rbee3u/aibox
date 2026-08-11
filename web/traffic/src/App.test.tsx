import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { ApiError } from "./api";
import {
  activeDetail,
  activeRecordList,
  activeSummary,
  completedDetail,
  completedSummary,
  completedSummaryFor,
  fakeApi,
  recordList,
  recordListFor,
  withIncompleteRequestBody,
  withRequestEncoding,
} from "./test/fixtures";
import type { TrafficApi } from "./types";

const zstdBytes = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderApp(overrides: Partial<TrafficApi> = {}) {
  return render(<App api={fakeApi(overrides)} />);
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
  action: "Delete selected" | "Delete all",
) {
  await user.click(screen.getByRole("button", { name: action }));
  await user.click(screen.getByRole("button", { name: "Delete permanently" }));
}

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("Traffic App", () => {
  it("renders resource links and concise record summaries", async () => {
    renderApp();

    await screen.findByText("AIBox Traffic");
    expect(screen.getByText("Inspect your LLM requests")).toBeInTheDocument();

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
    expect(within(banner).getByRole("combobox", { name: "Color theme" })).toHaveValue("system");

    const recordListPanel = screen.getByRole("complementary", { name: "Traffic records" });
    expect(
      within(recordListPanel).getByRole("heading", { name: "Traffic records", level: 2 }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(within(recordListPanel).getByRole("button", { name: "Delete all" })).toBeEnabled(),
    );
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
    expect(completedTarget).toHaveTextContent("api.example.test/v1/responses");
    expect(within(completedRow).queryByText(/stream=true/)).not.toBeInTheDocument();
    expect(completedEnded).toHaveTextContent("2026-08-06 12:00:01");
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
    renderApp();

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
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("storage unavailable", "SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("storage unavailable", "SecurityError");
    });

    renderApp();

    expect(screen.getByRole("combobox", { name: "Color theme" })).toHaveValue("system");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(screen.getByRole("separator", { name: "Resize Traffic records panel" })).toHaveAttribute(
      "aria-valuenow",
      "480",
    );
  });

  it("resizes the record panel with pointer and keyboard controls", () => {
    window.localStorage.setItem("aibox-traffic-list-width", "not-a-width");
    renderApp();

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
        ...completedSummary.protocol,
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
        ...completedSummary.protocol,
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
    renderApp({
      listRecords: vi.fn().mockResolvedValue(recordListFor([effective, requestedFallback, legacy])),
    });

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
    renderApp({ listRecords: vi.fn().mockResolvedValue(recordListFor([issueSummary])) });

    const row = await screen.findByRole("button", {
      name: "POST api.example.test/v1/responses",
    });
    expect(row).toHaveAccessibleDescription(
      /Record error: Server error\. Our servers are currently overloaded/,
    );
  });

  it("keeps Refresh enabled while a background list load is pending", async () => {
    const pendingList = deferred<typeof recordList>();
    renderApp({
      listRecords: vi.fn().mockReturnValue(pendingList.promise),
    });

    expect(screen.getByRole("button", { name: /Refresh/ })).toBeEnabled();
    pendingList.resolve(recordList);
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
  });

  it("marks the list refresh busy until a manual refresh completes", async () => {
    const refresh = deferred<typeof recordList>();
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockReturnValueOnce(refresh.promise);
    const user = userEvent.setup();
    renderApp({ listRecords });

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

    refresh.resolve(recordList);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeEnabled(),
    );
  });

  it("keeps Next clickable while a background list refresh is pending", async () => {
    vi.useFakeTimers();
    const firstPage = { ...recordList, has_next: true };
    const secondPage = recordListFor([completedSummary], { total: 73, deletable_count: 72 });
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
    renderApp({ listRecords });

    await flushEffects();
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    await advanceTimers(5000);
    expect(screen.getByRole("button", { name: /Next/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: /Next/ }));
    await flushEffects();
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith(2, expect.any(AbortSignal));
  });

  it("uses the browser API by default", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(Response.json(recordList));
    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
    expect(fetchMock).toHaveBeenCalledOnce();
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
    expect(screen.getByRole("button", { name: "Select page" })).toBeInTheDocument();
    expect(screen.getByText("0 selected")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Traffic records" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refresh traffic records" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
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

  it("selects and clears all completed records on the current page", async () => {
    const secondCompleted = completedSummaryFor(
      "0198-demo-completed-second",
      "second.example.test",
    );
    const user = userEvent.setup();
    renderApp({
      listRecords: vi
        .fn()
        .mockResolvedValue(recordListFor([activeSummary, completedSummary, secondCompleted])),
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

  it("disables selection and global deletion when no completed records exist", async () => {
    renderApp({ listRecords: vi.fn().mockResolvedValue(activeRecordList) });

    expect(await screen.findByRole("button", { name: "Select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
  });

  it("deletes one record without confirmation, locks deletion, clears detail, and restores focus", async () => {
    const secondCompleted = completedSummaryFor(
      "0198-demo-completed-second",
      "second.example.test",
    );
    const initial = recordListFor([completedSummary, secondCompleted]);
    const afterDelete = recordListFor([secondCompleted]);
    const deleteRequest = deferred<number>();
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(initial)
      .mockResolvedValue(afterDelete);
    const deleteRecords = vi
      .fn<TrafficApi["deleteRecords"]>()
      .mockReturnValue(deleteRequest.promise);
    const user = userEvent.setup();
    renderApp({ listRecords, deleteRecords });

    await openCompletedRecord(user);
    await screen.findByRole("region", { name: "Traffic record details" });
    await user.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const deleting = screen.getByRole("button", {
      name: "Deleting POST api.example.test/v1/responses",
    });
    expect(deleting).toBeDisabled();
    expect(deleting).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();

    act(() => deleteRequest.resolve(1));

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "POST api.example.test/v1/responses" }),
      ).not.toBeInTheDocument(),
    );
    expect(deleteRecords).toHaveBeenCalledWith([completedSummary.id]);
    expect(screen.getByText("Page 1 · 1 shown · 1 total")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a request" })).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Delete POST second.example.test/v1/responses" }),
      ).toHaveFocus(),
    );
    expect(screen.queryByText("Record deleted")).not.toBeInTheDocument();
  });

  it("keeps list navigation locked while a record deletion is pending", async () => {
    vi.useFakeTimers();
    const firstPage = recordListFor(recordList.records, {
      total: 51,
      deletable_count: 50,
      has_next: true,
    });
    const deleteRequest = deferred<number>();
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValue(recordListFor([activeSummary], { total: 50, deletable_count: 49 }));
    renderApp({
      listRecords,
      deleteRecords: vi.fn().mockReturnValue(deleteRequest.promise),
    });

    await flushEffects();
    fireEvent.click(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    );

    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    await advanceTimers(10_000);
    expect(listRecords).toHaveBeenCalledTimes(1);

    await act(async () => {
      deleteRequest.resolve(1);
      await deleteRequest.promise;
    });
    expect(listRecords).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeEnabled();
  });

  it("keeps a record when immediate deletion fails", async () => {
    const deleteRecords = vi
      .fn<TrafficApi["deleteRecords"]>()
      .mockRejectedValue(new Error("cannot delete record"));
    const user = userEvent.setup();
    renderApp({ deleteRecords });

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
    renderApp({ listRecords, deleteRecords: vi.fn().mockResolvedValue(1) });

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
    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toHaveFocus();
  });

  it("returns to the previous page when immediate deletion empties the current page", async () => {
    const secondPageSummary = completedSummaryFor("0198-demo-second-page", "second.example.test");
    const firstPage = recordListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = recordListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const emptySecondPage = recordListFor([], { total: 1, deletable_count: 1 });
    const firstPageAfterDelete = recordListFor([completedSummary]);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPageAfterDelete);
    const user = userEvent.setup();
    renderApp({ listRecords, deleteRecords: vi.fn().mockResolvedValue(1) });

    await user.click(await screen.findByRole("button", { name: "Next" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Delete POST second.example.test/v1/responses",
      }),
    );

    await screen.findByText("Page 1 · 1 shown · 1 total");
    expect(
      screen.getByRole("button", { name: "Delete POST api.example.test/v1/responses" }),
    ).toHaveFocus();
  });

  it("preserves the selected count across record pages", async () => {
    const secondPageSummary = completedSummaryFor("0198-demo-second-page", "second.example.test");
    const firstPage = recordListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = recordListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage);
    const user = userEvent.setup();
    renderApp({ listRecords });

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
    const firstPage = recordListFor([completedSummary], {
      total: 2,
      deletable_count: 2,
      has_next: true,
    });
    const secondPage = recordListFor([secondPageSummary], { total: 2, deletable_count: 2 });
    const afterDelete = recordListFor([]);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValue(afterDelete);
    const deleteRecords = vi.fn<TrafficApi["deleteRecords"]>().mockResolvedValue(2);
    const user = userEvent.setup();
    renderApp({ listRecords, deleteRecords });

    await selectCompletedRecord(user);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Select POST second.example.test/v1/responses",
      }),
    );
    await confirmDeletion(user, "Delete selected");

    await screen.findByText("Page 1 · 0 shown · 0 total");
    expect(screen.getByText("No traffic recorded yet.")).toBeInTheDocument();
    expect(deleteRecords).toHaveBeenCalledWith([completedSummary.id, secondPageSummary.id]);
    expect(listRecords).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });

  it("returns to the lowest selected page when selection starts on a later page", async () => {
    const page2Summary = completedSummaryFor("0198-demo-page-two", "two.example.test");
    const page3Summary = completedSummaryFor("0198-demo-page-three", "three.example.test");
    const pages = new Map([
      [1, recordListFor([completedSummary], { total: 3, deletable_count: 3, has_next: true })],
      [2, recordListFor([page2Summary], { total: 3, deletable_count: 3, has_next: true })],
      [3, recordListFor([page3Summary], { total: 3, deletable_count: 3 })],
    ]);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockImplementation((page = 1) => Promise.resolve(pages.get(page)!));
    const user = userEvent.setup();
    renderApp({ listRecords, deleteRecords: vi.fn().mockResolvedValue(2) });

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

    await waitFor(() => expect(listRecords).toHaveBeenLastCalledWith(2, expect.any(AbortSignal)));
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();
  });

  it("falls back when polling finds the current page empty", async () => {
    vi.useFakeTimers();
    const secondPageSummary = completedSummaryFor("0198-demo-poll-page", "poll.example.test");
    const firstPage = { ...recordList, has_next: true };
    const secondPage = recordListFor([secondPageSummary], { total: 51, deletable_count: 50 });
    const emptySecondPage = recordListFor([], { total: 50, deletable_count: 49 });
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(secondPage)
      .mockResolvedValueOnce(emptySecondPage)
      .mockResolvedValue(firstPage);
    renderApp({ listRecords });

    await flushEffects();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await flushEffects();
    expect(screen.getByText(/^Page 2 ·/)).toBeInTheDocument();

    await advanceTimers(5000);

    expect(screen.getByText(/^Page 1 ·/)).toBeInTheDocument();
    expect(listRecords).toHaveBeenLastCalledWith(1, expect.any(AbortSignal));
  });

  it("clears selection on Cancel, keeps focus safe, and ignores Escape", async () => {
    const user = userEvent.setup();
    renderApp();

    await selectCompletedRecord(user);
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await user.click(cancel);

    expect(screen.getByRole("button", { name: "Refresh traffic records" })).toBeInTheDocument();
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
    const dialog = screen.getByRole("dialog", { name: "Delete 1 selected record?" });
    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(dialog).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear page" })).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("keeps selection mode and selected ids when deletion fails", async () => {
    const user = userEvent.setup();
    renderApp({ deleteRecords: vi.fn().mockRejectedValue(new Error("delete failed")) });

    await selectCompletedRecord(user);
    await confirmDeletion(user, "Delete selected");

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
    renderApp({ listRecords });

    await flushEffects();
    expect(screen.getByRole("button", { name: "Select" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    await advanceTimers(7500);

    expect(listRecords).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "Refresh traffic records" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await flushEffects();
    expect(listRecords).toHaveBeenCalledTimes(2);
  });

  it("loads request and response Bodies only when their tabs are selected", async () => {
    const encoder = new TextEncoder();
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, kind) =>
      Promise.resolve({
        bytes: encoder.encode(kind === "request" ? "request body" : "data: response body\n\n"),
        nextOffset: kind === "request" ? 12 : 21,
      }),
    );
    const loadEventTimings = vi.fn<TrafficApi["loadEventTimings"]>().mockResolvedValue({
      state: "available",
      events: [{ sequence: 0, completed_at_ns: "900000000" }],
      next_sequence: 1,
      warning: null,
    });
    const user = userEvent.setup();
    renderApp({ loadBody, loadEventTimings });

    await openCompletedRecord(user);
    const detail = screen.getByRole("region", { name: "Traffic record details" });
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

  it("keeps the detail view and reports a body read failure", async () => {
    const user = userEvent.setup();
    renderApp({ loadBody: vi.fn().mockRejectedValue(new Error("body unavailable")) });

    await openCompletedRecord(user);
    await user.click(await screen.findByRole("tab", { name: "Request" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("body unavailable");
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getByRole("status")).toHaveTextContent("Original Body unavailable.");
  });

  it("loads zstd decoded Source only after the complete raw Body is available", async () => {
    const decoded = new TextEncoder().encode('{"model":"gpt-5.6-sol"}');
    const detail = {
      ...withRequestEncoding(completedDetail, "zstd"),
      request_body_bytes: zstdBytes.length,
    };
    const loadDecodedBody = vi.fn<TrafficApi["loadDecodedBody"]>().mockResolvedValue(decoded);
    renderApp({
      getRecord: vi.fn().mockResolvedValue(detail),
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
      .fn<TrafficApi["loadDecodedBody"]>()
      .mockRejectedValueOnce(new Error("decode failed"))
      .mockReturnValueOnce(retry.promise);
    const user = userEvent.setup();
    renderApp({
      getRecord: vi.fn().mockResolvedValue(detail),
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
    const getRecord = vi.fn<TrafficApi["getRecord"]>().mockImplementation(() =>
      Promise.resolve({
        ...zstdDetail,
        request: { ...zstdDetail.request },
        summary: { ...zstdDetail.summary },
      }),
    );
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, _kind, offset) =>
      Promise.resolve({
        bytes: offset === 0 ? zstdBytes : new Uint8Array(),
        nextOffset: zstdBytes.length,
      }),
    );
    let decodedSignal: AbortSignal | undefined;
    const decodedRequest = deferred<Uint8Array>();
    const loadDecodedBody = vi
      .fn<TrafficApi["loadDecodedBody"]>()
      .mockImplementation((_id, _kind, signal) => {
        decodedSignal = signal;
        return decodedRequest.promise;
      });
    renderApp({
      listRecords: vi.fn().mockResolvedValue(activeRecordList),
      getRecord,
      loadBody,
      loadDecodedBody,
    });

    await openActiveRequestBody();
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);

    await advanceTimers(3000);
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(loadBody).toHaveBeenCalledTimes(1);
    expect(loadDecodedBody).toHaveBeenCalledTimes(1);
    expect(decodedSignal?.aborted).toBe(false);

    await act(async () => {
      decodedRequest.resolve(decoded);
      await decodedRequest.promise;
    });
    expect(screen.getByText('"ready"')).toBeInTheDocument();
  });

  it("ignores a stale zstd decode failure after selecting another record", async () => {
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
    const loadDecodedBody = vi
      .fn<TrafficApi["loadDecodedBody"]>()
      .mockReturnValueOnce(firstDecode.promise)
      .mockReturnValueOnce(secondDecode.promise);
    renderApp({
      getRecord: vi
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

  it("clears a completed selection when delete-all overlaps its pending detail request", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockResolvedValue(activeRecordList);
    const detailRequest = deferred<typeof completedDetail>();
    const user = userEvent.setup();
    renderApp({
      listRecords,
      getRecord: vi.fn().mockReturnValue(detailRequest.promise),
      deleteAll: vi.fn().mockResolvedValue(1),
    });

    await openCompletedRecord(user);
    expect(screen.getByText("Loading record…")).toBeInTheDocument();
    await confirmDeletion(user, "Delete all");

    await screen.findByRole("heading", { name: "Select a request" });
    expect(screen.queryByText("Loading record…")).not.toBeInTheDocument();
  });

  it("keeps an active selection when delete-all overlaps its pending detail request", async () => {
    const detailRequest = deferred<typeof activeDetail>();
    const deleteAll = vi.fn<TrafficApi["deleteAll"]>().mockResolvedValue(1);
    const user = userEvent.setup();
    renderApp({
      getRecord: vi.fn().mockReturnValue(detailRequest.promise),
      deleteAll,
    });

    await user.click(await screen.findByRole("button", { name: "GET stream.example.test/events" }));
    expect(screen.getByText("Loading record…")).toBeInTheDocument();
    await confirmDeletion(user, "Delete all");

    await waitFor(() => expect(deleteAll).toHaveBeenCalledWith(1));
    expect(screen.getByText("Loading record…")).toBeInTheDocument();
    detailRequest.resolve(activeDetail);
  });

  it("shows list failures and retries in place", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockRejectedValueOnce(new Error("cannot scan Traffic Records"))
      .mockResolvedValue(recordList);
    const user = userEvent.setup();
    renderApp({ listRecords });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("cannot scan Traffic Records");
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
  });

  it("clears a record that disappears before its detail loads", async () => {
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockRejectedValue(new ApiError("Traffic Record not found", 404));
    const user = userEvent.setup();
    renderApp({ getRecord });

    await openCompletedRecord(user);

    await screen.findByRole("heading", { name: "Select a request" });
    expect(screen.getByRole("alert")).toHaveTextContent("Traffic Record not found");
  });

  it("keeps simultaneous list and detail failures in their own regions", async () => {
    vi.useFakeTimers();
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockRejectedValue(new Error("list polling failed"));
    renderApp({
      listRecords,
      getRecord: vi.fn().mockRejectedValue(new Error("detail loading failed")),
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

  it("keeps a post-delete list refresh failure visible", async () => {
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockResolvedValueOnce(recordList)
      .mockRejectedValue(new Error("cannot refresh Traffic Records"));
    const user = userEvent.setup();
    renderApp({ listRecords, deleteAll: vi.fn().mockResolvedValue(1) });

    await waitFor(() => expect(screen.getByRole("button", { name: "Delete all" })).toBeEnabled());
    await confirmDeletion(user, "Delete all");

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
    const initial = deferred<typeof recordList>();
    const refreshed = recordListFor([completedSummary]);
    const listRecords = vi
      .fn<TrafficApi["listRecords"]>()
      .mockReturnValueOnce(initial.promise)
      .mockResolvedValueOnce(refreshed);
    const api = fakeApi({ listRecords });
    const replacementApi = fakeApi({ listRecords: vi.fn().mockResolvedValue(refreshed) });
    const { rerender } = render(<App api={api} />);

    await flushEffects();
    rerender(<App api={replacementApi} />);
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });

    await act(async () => {
      initial.resolve(recordList);
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
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockResolvedValueOnce({
        ...completedDetail,
        request: { ...completedDetail.request, id: activeSummary.id },
      });
    renderApp({
      listRecords: vi.fn().mockResolvedValue(activeRecordList),
      getRecord,
    });

    await openActiveRecord();
    const detail = screen.getByRole("region", { name: "Traffic record details" });
    expect(within(detail).getAllByText("Waiting").length).toBeGreaterThan(0);

    await advanceTimers(3000);
    expect(getRecord).toHaveBeenCalledTimes(2);
    expect(within(detail).queryAllByText("Waiting")).toHaveLength(0);
    expect(within(detail).getByLabelText("HTTP/2 200 OK")).toBeInTheDocument();
  });

  it("stops polling an active detail after a deterministic not-found response", async () => {
    vi.useFakeTimers();
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockResolvedValueOnce(activeDetail)
      .mockRejectedValue(new ApiError("Traffic Record not found", 404));
    renderApp({
      listRecords: vi.fn().mockResolvedValue(activeRecordList),
      getRecord,
    });

    await openActiveRecord();
    await advanceTimers(3000);

    expect(screen.getByRole("alert")).toHaveTextContent("Traffic Record not found");
    expect(screen.getByRole("heading", { name: "Select a request" })).toBeInTheDocument();
    expect(getRecord).toHaveBeenCalledTimes(2);
    await advanceTimers(9000);
    expect(getRecord).toHaveBeenCalledTimes(2);
  });

  it("stops polling an active Body after a deterministic not-found response", async () => {
    vi.useFakeTimers();
    const incompleteDetail = withIncompleteRequestBody(activeDetail);
    const loadBody = vi
      .fn<TrafficApi["loadBody"]>()
      .mockRejectedValue(new ApiError("Traffic Record not found", 404));
    renderApp({
      listRecords: vi.fn().mockResolvedValue(activeRecordList),
      getRecord: vi.fn().mockResolvedValue(incompleteDetail),
      loadBody,
    });

    await openActiveRequestBody();

    expect(screen.getByRole("alert")).toHaveTextContent("Traffic Record not found");
    expect(screen.getByRole("region", { name: "Traffic record details" })).toBeInTheDocument();
    expect(loadBody).toHaveBeenCalledTimes(1);
    await advanceTimers(9000);
    expect(loadBody).toHaveBeenCalledTimes(1);
  });

  it("does not overlap active body polls", async () => {
    vi.useFakeTimers();
    const encoder = new TextEncoder();
    const requestPoll = deferred<{ bytes: Uint8Array; nextOffset: number }>();
    const loadBody = vi.fn<TrafficApi["loadBody"]>().mockImplementation((_id, kind, offset) => {
      if (offset === 0) return Promise.resolve({ bytes: encoder.encode(kind), nextOffset: 1 });
      return requestPoll.promise;
    });
    const getRecord = vi
      .fn<TrafficApi["getRecord"]>()
      .mockResolvedValue(withIncompleteRequestBody(activeDetail));
    renderApp({
      listRecords: vi.fn().mockResolvedValue(activeRecordList),
      getRecord,
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
