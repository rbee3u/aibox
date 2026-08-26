import { describe, expect, it, vi } from "vitest";
import { composeControlApi } from "@/api/connect";
import { ControlApi } from "@/api/transport";

const tenant = { kind: "managed", name: "work" } as const;
const target = {
  tenant,
  agent: "codex",
  current: false,
  config: "review",
  file: "config.toml",
} as const;

function connected() {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockImplementation(() => Promise.resolve(Response.json({})));
  const api = composeControlApi(
    new ControlApi({ version: "1", csrf_token: "token", listen: "127.0.0.1:3000" }, fetchMock),
  );
  return { api, fetchMock };
}

describe("Control API endpoints", () => {
  it("owns semantic paths and Tenant encoding for scoped reads", async () => {
    const { api, fetchMock } = connected();

    await api.overview.loadSessionSummary(tenant, "codex");
    await api.tenants.listComponents(tenant);
    await api.tenants.latestComponents();
    await api.tenants.checkLatestComponents();
    await api.sessions.loadSessionEvidence(tenant, "codex", "session/1", "entry 1", "10:2");

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/_aibox/api/sessions/summary?tenant=managed%3Awork&agent=codex",
      "/_aibox/api/components?tenant=managed%3Awork",
      "/_aibox/api/components/latest",
      "/_aibox/api/components/latest/check",
      "/_aibox/api/sessions/evidence?tenant=managed%3Awork&agent=codex&id=session%2F1&entry=entry+1&snapshot=10%3A2",
    ]);
    expect(JSON.parse(fetchMock.mock.calls[3][1]?.body as string)).toEqual({});
  });

  it("sends the Config file selector and native wire fields", async () => {
    const { api, fetchMock } = connected();

    await api.configs.revealConfigFile(target);
    await api.configs.saveConfigFile(target, {
      revision: "rev-1",
      contentBase64: "Y29udGVudA==",
      visualOptions: [{ path: "model", included: true, value: "gpt-5" }],
      customProvider: {
        included: true,
        name: "local",
        base_url: "http://127.0.0.1:3000",
        proxy_routed: true,
      },
    });

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/_aibox/api/configs/reveal",
      "/_aibox/api/configs/save",
    ]);
    expect(JSON.parse(fetchMock.mock.calls[0][1]?.body as string)).toEqual({
      tenant: "managed:work",
      agent: "codex",
      current: false,
      config: "review",
      file: "config.toml",
    });
    expect(JSON.parse(fetchMock.mock.calls[1][1]?.body as string)).toEqual({
      tenant: "managed:work",
      agent: "codex",
      current: false,
      config: "review",
      file: "config.toml",
      revision: "rev-1",
      content_base64: "Y29udGVudA==",
      visual_options: [{ path: "model", included: true, value: "gpt-5" }],
      custom_provider: {
        included: true,
        name: "local",
        base_url: "http://127.0.0.1:3000",
        proxy_routed: true,
      },
    });
  });

  it("dispatches Session detail frames and requires a completion frame", async () => {
    const frames = [
      '{"type":"meta","meta":{"id":"session-1","title":"Hello","start_ts":"now","transcript_path":".codex/x.jsonl"}}',
      '{"type":"message","message":{"entry_ids":["line-1"],"role":"user","timestamp":"now","text":"hello"}}',
      '{"type":"complete","stats":{"start_ts":"now","last_event_ts":"later","message_count":1,"tool_count":0,"entry_count":1,"malformed_count":0,"unsupported_count":0,"hidden_internal_count":0,"file_size":10,"snapshot":"10:1"},"warnings":[]}',
    ];
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockImplementation(() => Promise.resolve(new Response(`${frames.join("\n")}\n`)));
    const api = composeControlApi(
      new ControlApi({ version: "1", csrf_token: "token", listen: "127.0.0.1:3000" }, fetchMock),
    );
    const messages: string[] = [];
    let meta = "";
    let complete = 0;

    await api.sessions.streamSessionDetail(tenant, "codex", "session-1", {
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

  it("fails a Session detail stream that ends before completion", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockImplementation(() =>
        Promise.resolve(
          new Response(
            '{"type":"message","message":{"entry_ids":["line-1"],"role":"user","timestamp":"now","text":"hello"}}\n',
          ),
        ),
      );
    const api = composeControlApi(
      new ControlApi({ version: "1", csrf_token: "token", listen: "127.0.0.1:3000" }, fetchMock),
    );

    await expect(
      api.sessions.streamSessionDetail(tenant, "codex", "session-1", {
        onMeta: () => undefined,
        onMessage: () => undefined,
        onTool: () => undefined,
        onEvidence: () => undefined,
        onComplete: () => undefined,
      }),
    ).rejects.toThrow("Session detail stream ended before completion");
  });
});
