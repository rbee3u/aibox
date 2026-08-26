import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import { ApiError } from "@/api/transport";
import {
  activeDetail,
  activeRequestList,
  activeSummary,
  completedDetail,
  completedSummary,
  withIncompleteRequestBody,
  withRequestEncoding,
} from "@/features/requests/testFixtures";
import {
  advanceTimers,
  openActiveRecord,
  openActiveRequestBody,
  openCompletedRecord,
  renderApp,
  zstdBytes,
} from "@/features/requests/testHarness";
import { deferred } from "@/test/deferred";

describe("Requests page body inspection", () => {
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
