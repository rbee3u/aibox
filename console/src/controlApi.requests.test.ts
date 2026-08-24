import { describe, expect, it, vi } from "vitest";
import { ApiError, composeControlApi, ControlApi } from "./controlApi";
import type { RequestsApi } from "./types";

function requestsApi(fetchImpl: typeof fetch): RequestsApi {
  return composeControlApi(
    new ControlApi({ version: "1", csrf_token: "csrf", listen: "127.0.0.1:9923" }, fetchImpl),
  ).requests;
}

describe("Request API client", () => {
  it("invokes fetch with the Window receiver", async () => {
    const fetchMock = vi.fn(function (this: Window) {
      if (this !== window) throw new TypeError("Illegal invocation");
      return Promise.resolve(
        Response.json({ requests: [], total: 0, deletable_count: 0, has_next: false }),
      );
    }) as typeof fetch;
    const api = requestsApi(fetchMock);

    await expect(api.listRequests()).resolves.toMatchObject({ requests: [] });
  });

  it("uses one-based page queries", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockImplementation(() =>
        Promise.resolve(
          Response.json({ requests: [], total: 0, deletable_count: 0, has_next: false }),
        ),
      );
    const api = requestsApi(fetchMock);

    await api.listRequests(1);
    await api.listRequests(3);

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/_aibox/api/requests",
      "/_aibox/api/requests?page=3",
    ]);
  });

  it("encodes Request ids and keeps read requests cache-free", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(Response.json({}));
    const api = requestsApi(fetchMock);

    await api.getRequest("request/id");

    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/_aibox/api/requests/request%2Fid");
    expect(init).toMatchObject({ cache: "no-store" });
    expect(init?.headers).toBeUndefined();
  });

  it("forwards cancellation to every API operation", async () => {
    const responses = [
      Response.json({ requests: [], total: 0, deletable_count: 0 }),
      Response.json({}),
      new Response(new Uint8Array()),
      new Response(new Uint8Array()),
      Response.json({ state: "unavailable", events: [], next_sequence: 0 }),
      Response.json({ deleted: 0 }),
    ];
    let responseIndex = 0;
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockImplementation(() => Promise.resolve(responses[responseIndex++]));
    const signal = new AbortController().signal;
    const api = requestsApi(fetchMock);

    await Promise.all([
      api.listRequests(2, signal),
      api.getRequest("request", signal),
      api.loadBody("request", "request", 0, signal),
      api.loadDecodedBody("request", "response", signal),
      api.loadEventTimings("request", 0, signal),
      api.deleteRequests([], signal),
    ]);

    expect(fetchMock.mock.calls).toHaveLength(6);
    for (const [, init] of fetchMock.mock.calls) {
      expect(init?.signal).toBe(signal);
    }
  });

  it("sends selected Requests as JSON", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(Response.json({ deleted: 2 }));
    const api = requestsApi(fetchMock);

    await expect(api.deleteRequests(["one", "two"])).resolves.toBe(2);
    expect(fetchMock).toHaveBeenCalledWith(
      "/_aibox/api/requests/delete",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ ids: ["one", "two"] }),
        cache: "no-store",
      }),
    );
    const [, init] = fetchMock.mock.calls[0];
    const headers = new Headers(init?.headers);
    expect(headers.get("Content-Type")).toBe("application/json");
  });

  it.each([
    [
      "JSON error",
      Response.json(
        { error: "active Requests cannot be deleted" },
        {
          status: 409,
          statusText: "Conflict",
        },
      ),
      { message: "active Requests cannot be deleted", status: 409 },
    ],
    [
      "HTTP status when the body is not JSON",
      new Response("Bad Gateway", { status: 502, statusText: "Bad Gateway" }),
      { message: "502 Bad Gateway", status: 502 },
    ],
  ])("surfaces the server's %s", async (_case, response, expectedError) => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(response);
    const api = requestsApi(fetchMock);

    const request = api.deleteRequests(["active"]);
    await expect(request).rejects.toBeInstanceOf(ApiError);
    await expect(request).rejects.toMatchObject({ name: "ApiError", ...expectedError });
  });

  it.each([
    ["matching", "9"],
    ["missing", null],
    ["non-numeric", "not-a-number"],
    ["out-of-range", String(Number.MAX_SAFE_INTEGER + 1)],
    ["mismatched", "12"],
  ])("loads Body chunks safely with a %s offset header", async (_case, advertisedOffset) => {
    const headers = new Headers();
    if (advertisedOffset !== null) {
      headers.set("X-Aibox-Request-Next-Offset", advertisedOffset);
    }
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response(new Uint8Array([3, 4]), { headers }));
    const api = requestsApi(fetchMock);

    await expect(api.loadBody("request/id", "response", 7)).resolves.toEqual({
      bytes: new Uint8Array([3, 4]),
      nextOffset: 9,
    });
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/api/requests/request%2Fid/response-body?offset=7",
    );
  });

  it("loads decoded Body bytes without a raw offset", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response(new Uint8Array([1, 2, 3])));
    const api = requestsApi(fetchMock);

    await expect(api.loadDecodedBody("request/id", "request")).resolves.toEqual(
      new Uint8Array([1, 2, 3]),
    );
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/api/requests/request%2Fid/request-body-decoded",
    );
  });

  it("loads SSE Event timings incrementally", async () => {
    const payload = {
      state: "partial",
      events: [{ sequence: 3, completed_at_ns: "123000000" }],
      next_sequence: 4,
      warning: "index is incomplete",
    };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(Response.json(payload));
    const api = requestsApi(fetchMock);

    await expect(api.loadEventTimings("request/id", 3)).resolves.toEqual(payload);
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/api/requests/request%2Fid/response-event-timings?after_sequence=3",
    );
  });
});
