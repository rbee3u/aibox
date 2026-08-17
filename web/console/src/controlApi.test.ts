import { describe, expect, it, vi } from "vitest";
import { ControlApi } from "./controlApi";

describe("Control API client", () => {
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

  it("streams prompt records and returns terminal warnings", async () => {
    const encoded = new TextEncoder().encode(
      '{"type":"prompt","prompt":{"timestamp":"now","text":"hello"}}\n' +
        '{"type":"complete","id":"session-1","warnings":["partial"]}\n',
    );
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(encoded.subarray(0, 17));
            controller.enqueue(encoded.subarray(17));
            controller.close();
          },
        }),
      ),
    );
    const api = new ControlApi({ version: "1", csrf_token: "token" }, fetchMock);
    const prompts: string[] = [];

    const complete = await api.streamSession("/sessions", (prompt) => prompts.push(prompt.text));

    expect(prompts).toEqual(["hello"]);
    expect(complete).toEqual({ id: "session-1", warnings: ["partial"] });
  });
});
