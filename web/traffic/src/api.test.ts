import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, createTrafficApi } from "./api";

describe("Traffic API client", () => {
  beforeEach(() => {
    document.head.innerHTML = '<meta name="aibox-csrf" content="test-token">';
  });

  it("adds CSRF and JSON headers to mutating requests", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify({ deleted: 2 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );
    const api = createTrafficApi(fetchMock);

    await expect(api.deleteRecords(["one", "two"])).resolves.toBe(2);
    const [, init] = fetchMock.mock.calls[0];
    const headers = new Headers(init?.headers);
    expect(headers.get("X-Aibox-Traffic-CSRF")).toBe("test-token");
    expect(headers.get("Content-Type")).toBe("application/json");
    expect(init?.body).toBe('{"ids":["one","two"]}');
  });

  it("surfaces the server's JSON error", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify({ error: "active Traffic Records cannot be deleted" }), {
        status: 409,
        statusText: "Conflict",
      }),
    );
    const api = createTrafficApi(fetchMock);

    await expect(api.deleteRecords(["active"])).rejects.toEqual(
      new ApiError("active Traffic Records cannot be deleted", 409),
    );
  });

  it("passes body offsets and reads the authoritative next offset", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(new Uint8Array([3, 4]), {
        headers: { "X-Aibox-Traffic-Next-Offset": "9" },
      }),
    );
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
