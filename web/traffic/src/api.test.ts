import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, createTrafficApi } from "./api";
import type { TrafficApi } from "./types";

describe("Traffic API client", () => {
  beforeEach(() => {
    document.head.innerHTML = '<meta name="aibox-csrf" content="test-token">';
  });

  it("uses one-based page queries", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockImplementation(() =>
        Promise.resolve(
          new Response(
            JSON.stringify({ records: [], total: 0, deletable_count: 0, has_next: false }),
          ),
        ),
      );
    const api = createTrafficApi(fetchMock);

    await api.listRecords(1);
    await api.listRecords(3);

    expect(fetchMock.mock.calls[0]?.[0]).toBe("/_aibox/traffic/api/records");
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/_aibox/traffic/api/records?page=3");
  });

  const mutations = [
    {
      name: "selected records",
      run: (api: TrafficApi) => api.deleteRecords(["one", "two"]),
      path: "/_aibox/traffic/api/records/delete",
      body: '{"ids":["one","two"]}',
    },
    {
      name: "all records",
      run: (api: TrafficApi) => api.deleteAll(7),
      path: "/_aibox/traffic/api/records/delete-all",
      body: '{"expected_deletable_count":7}',
    },
  ];

  it.each(mutations)("adds CSRF and JSON headers to $name", async ({ run, path, body }) => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify({ deleted: 2 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );
    const api = createTrafficApi(fetchMock);

    await expect(run(api)).resolves.toBe(2);
    expect(fetchMock).toHaveBeenCalledWith(
      path,
      expect.objectContaining({ method: "POST", body, cache: "no-store" }),
    );
    const [, init] = fetchMock.mock.calls[0];
    const headers = new Headers(init?.headers);
    expect(headers.get("X-Aibox-Traffic-CSRF")).toBe("test-token");
    expect(headers.get("Content-Type")).toBe("application/json");
  });

  it.each([
    [
      "JSON error",
      new Response(JSON.stringify({ error: "active Traffic Records cannot be deleted" }), {
        status: 409,
        statusText: "Conflict",
      }),
      new ApiError("active Traffic Records cannot be deleted", 409),
    ],
    [
      "HTTP status when the body is not JSON",
      new Response("Bad Gateway", { status: 502, statusText: "Bad Gateway" }),
      new ApiError("502 Bad Gateway", 502),
    ],
  ])("surfaces the server's %s", async (_case, response, expectedError) => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(response);
    const api = createTrafficApi(fetchMock);

    await expect(api.deleteRecords(["active"])).rejects.toEqual(expectedError);
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
      headers.set("X-Aibox-Traffic-Next-Offset", advertisedOffset);
    }
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response(new Uint8Array([3, 4]), { headers }));
    const api = createTrafficApi(fetchMock);

    await expect(api.loadBody("record/id", "response", 7)).resolves.toEqual({
      bytes: new Uint8Array([3, 4]),
      nextOffset: 9,
    });
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/traffic/api/records/record%2Fid/response-body?offset=7",
    );
  });

  it("loads decoded Body bytes without a raw offset", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(new Response(new Uint8Array([1, 2, 3])));
    const api = createTrafficApi(fetchMock);

    await expect(api.loadDecodedBody("record/id", "request")).resolves.toEqual(
      new Uint8Array([1, 2, 3]),
    );
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/traffic/api/records/record%2Fid/request-body-decoded",
    );
  });

  it("loads SSE Event timings incrementally", async () => {
    const payload = {
      state: "partial",
      events: [{ sequence: 3, completed_at_ns: "123000000" }],
      next_sequence: 4,
      warning: "index is incomplete",
    };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify(payload), {
        headers: { "Content-Type": "application/json" },
      }),
    );
    const api = createTrafficApi(fetchMock);

    await expect(api.loadEventTimings("record/id", 3)).resolves.toEqual(payload);
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/_aibox/traffic/api/records/record%2Fid/response-event-timings?after_sequence=3",
    );
  });
});
