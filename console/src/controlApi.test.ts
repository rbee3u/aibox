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

  it("streams Session detail frames across chunk boundaries", async () => {
    const encoded = new TextEncoder().encode(
      '{"type":"meta","meta":{"id":"session-1","title":"Hello","start_ts":"now","transcript_path":".codex/x.jsonl"}}\n' +
        '{"type":"message","message":{"entry_ids":["line-1"],"role":"user","timestamp":"now","text":"hello"}}\n' +
        '{"type":"complete","stats":{"start_ts":"now","last_event_ts":"later","message_count":1,"tool_count":0,"entry_count":1,"malformed_count":0,"unsupported_count":0,"hidden_internal_count":0,"file_size":10,"snapshot":"10:1"},"warnings":[]}\n',
    );
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(encoded.subarray(0, 41));
            controller.enqueue(encoded.subarray(41));
            controller.close();
          },
        }),
      ),
    );
    const api = new ControlApi({ version: "1", csrf_token: "token" }, fetchMock);
    const messages: string[] = [];
    let meta = "";
    let complete = 0;
    await api.streamSessionDetail("/_aibox/api/sessions/detail", {
      onMeta: (value) => {
        meta = value.id;
      },
      onMessage: (value) => messages.push(value.text),
      onTool: () => undefined,
      onEvidence: () => undefined,
      onComplete: () => {
        complete += 1;
      },
    });

    expect(meta).toBe("session-1");
    expect(messages).toEqual(["hello"]);
    expect(complete).toBe(1);
  });
});
