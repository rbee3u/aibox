import { describe, expect, it, vi } from "vitest";
import { ControlApi } from "@/api/transport";

function client(fetchImpl: typeof fetch) {
  return new ControlApi({ version: "1", csrf_token: "token", listen: "127.0.0.1:3000" }, fetchImpl);
}

describe("Control API transport", () => {
  it("invokes fetch with the Window receiver", async () => {
    const fetchMock = vi.fn(function (this: Window) {
      if (this !== window) throw new TypeError("Illegal invocation");
      return Promise.resolve(Response.json({ version: "1.2.3", csrf_token: "token-1" }));
    }) as typeof fetch;

    await expect(ControlApi.connect(fetchMock)).resolves.toMatchObject({
      bootstrap: { version: "1.2.3", csrf_token: "token-1" },
    });
  });

  it("loads bootstrap and protects JSON mutations with the startup token", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ version: "1.2.3", csrf_token: "token-1" }))
      .mockResolvedValueOnce(Response.json({ created: "work" }));
    const api = await ControlApi.connect(fetchMock);

    await api.post("/_aibox/api/tenants", { name: "work" });

    expect(fetchMock.mock.calls[0][0]).toBe("/_aibox/api/bootstrap");
    const [path, init] = fetchMock.mock.calls[1];
    expect(path).toBe("/_aibox/api/tenants");
    expect(init?.method).toBe("POST");
    expect(new Headers(init?.headers).get("X-Aibox-Csrf")).toBe("token-1");
    expect(init?.body).toBe('{"name":"work"}');
  });

  it("reads NDJSON records across chunk boundaries", async () => {
    const encoded = new TextEncoder().encode('{"value":"first"}\n{"value":"second"}\n');
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(encoded.subarray(0, 11));
            controller.enqueue(encoded.subarray(11));
            controller.close();
          },
        }),
      ),
    );
    const records: string[] = [];

    await client(fetchMock).streamNdjson<{ value: string }>("/stream", (record) =>
      records.push(record.value),
    );

    expect(records).toEqual(["first", "second"]);
  });

  it("propagates a record handler failure out of the stream", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response('{"value":"first"}\n'));

    await expect(
      client(fetchMock).streamNdjson<{ value: string }>("/stream", () => {
        throw new Error("rejected frame");
      }),
    ).rejects.toThrow("rejected frame");
  });

  it("reports a non-OK response as an API error", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(Response.json({ error: "not found" }, { status: 404 }));

    await expect(client(fetchMock).get("/missing")).rejects.toMatchObject({
      name: "ApiError",
      status: 404,
      message: "not found",
    });
  });
});
