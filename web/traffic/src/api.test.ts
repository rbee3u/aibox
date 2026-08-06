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
});
