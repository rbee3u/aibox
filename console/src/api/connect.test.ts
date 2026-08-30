import { describe, expect, it, vi } from "vitest";
import { composeControlApi } from "@/api/connect";
import { ControlApi } from "@/api/transport";
import { controlMethod, controlRoute, type ControlRouteKey } from "@/test/controlRoutes";

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

function expectRouteCalls(
  fetchMock: ReturnType<typeof vi.fn<typeof fetch>>,
  expected: Array<{ key: ControlRouteKey; path: string }>,
) {
  expect(
    fetchMock.mock.calls.map(([path, init]) => ({
      path,
      method: init?.method ?? "GET",
    })),
  ).toEqual(
    expected.map(({ key, path }) => ({
      path,
      method: controlMethod(key),
    })),
  );
}

describe("Control API endpoints", () => {
  it("owns semantic paths and Tenant encoding for scoped reads", async () => {
    const { api, fetchMock } = connected();

    await api.overview.loadSessionSummary(tenant, "codex");
    await api.tenants.listComponents(tenant);
    await api.tenants.latestComponents();
    await api.tenants.checkLatestComponents();
    await api.sessions.loadSessionEvidence(tenant, "codex", "session/1", "entry 1", "10:2");

    expectRouteCalls(fetchMock, [
      {
        key: "sessions_summary",
        path: controlRoute("sessions_summary", {}, "tenant=managed%3Awork&agent=codex"),
      },
      {
        key: "components_list",
        path: controlRoute("components_list", {}, "tenant=managed%3Awork"),
      },
      { key: "components_latest", path: controlRoute("components_latest") },
      {
        key: "components_latest_check",
        path: controlRoute("components_latest_check"),
      },
      {
        key: "sessions_evidence",
        path: controlRoute(
          "sessions_evidence",
          {},
          "tenant=managed%3Awork&agent=codex&id=session%2F1&entry=entry+1&snapshot=10%3A2",
        ),
      },
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

    expectRouteCalls(fetchMock, [
      { key: "configs_reveal", path: controlRoute("configs_reveal") },
      { key: "configs_save", path: controlRoute("configs_save") },
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

  it("owns feature mutation paths, scoped queries, and snake-case bodies", async () => {
    const { api, fetchMock } = connected();

    await api.configs.listConfigs(tenant, "codex");
    await api.configs.diagnoseConfigFile(target, "YQ==");
    await api.configs.createConfig(tenant, "codex", "review");
    await api.configs.applyConfig(tenant, "codex", "review");
    await api.configs.deleteConfigs(tenant, "codex", ["review"]);
    await api.configs.previewCredentialPropagation();
    await api.configs.executeCredentialPropagation("plan-1");
    await api.tenants.createTenant("work");
    await api.tenants.deleteTenants(["work"]);
    await api.tenants.mutateComponent(tenant, "python", false, null);
    await api.sessions.listSessions(tenant, "claude");
    await api.sessions.deleteSessions(tenant, "claude", ["session-1"]);
    await api.overview.buildImage(true);

    expectRouteCalls(fetchMock, [
      {
        key: "configs_list",
        path: controlRoute("configs_list", {}, "tenant=managed%3Awork&agent=codex"),
      },
      { key: "configs_diagnose", path: controlRoute("configs_diagnose") },
      { key: "configs_create", path: controlRoute("configs_create") },
      { key: "configs_apply", path: controlRoute("configs_apply") },
      { key: "configs_delete", path: controlRoute("configs_delete") },
      {
        key: "configs_propagate_preview",
        path: controlRoute("configs_propagate_preview"),
      },
      {
        key: "configs_propagate_execute",
        path: controlRoute("configs_propagate_execute"),
      },
      { key: "tenants_create", path: controlRoute("tenants_create") },
      { key: "tenants_delete", path: controlRoute("tenants_delete") },
      { key: "components_remove", path: controlRoute("components_remove") },
      {
        key: "sessions_list",
        path: controlRoute("sessions_list", {}, "tenant=managed%3Awork&agent=claude"),
      },
      { key: "sessions_delete", path: controlRoute("sessions_delete") },
      { key: "operations_build", path: controlRoute("operations_build") },
    ]);
    const bodies = fetchMock.mock.calls.map(([, init]) =>
      init?.body ? (JSON.parse(init.body as string) as unknown) : undefined,
    );
    expect(bodies[1]).toMatchObject({
      tenant: "managed:work",
      agent: "codex",
      config: "review",
      content_base64: "YQ==",
    });
    expect(bodies[4]).toEqual({
      tenant: "managed:work",
      agent: "codex",
      configs: ["review"],
      all: false,
      confirmation: "review",
    });
    expect(bodies[6]).toEqual({ plan_id: "plan-1" });
    expect(bodies[8]).toEqual({ names: ["work"], all: false, confirmation: "work" });
    expect(bodies[9]).toEqual({
      tenant: "managed:work",
      component: "python",
      version: null,
    });
    expect(bodies[11]).toEqual({
      tenant: "managed:work",
      agent: "claude",
      ids: ["session-1"],
      all: false,
      confirmation: "",
    });
    expect(bodies[12]).toEqual({ force: true });
  });

  it("normalizes Rust-owned Config, Component, and Topology wire values", async () => {
    const { api, fetchMock } = connected();
    fetchMock
      .mockResolvedValueOnce(
        Response.json({
          file: "config.toml",
          exists: true,
          revision: "rev-1",
          content_base64: "",
          visual_options: [
            {
              path: "model",
              label: "Model",
              description: "Model",
              group: "Model",
              value_kind: "string",
              enum_values: [],
              sensitive: false,
              required: true,
              included: true,
              value: "gpt-5",
            },
          ],
          custom_provider: null,
          visual_error: null,
          warnings: [],
          auth: null,
          linked_file: null,
        }),
      )
      .mockResolvedValueOnce(
        Response.json([
          {
            kind: "python",
            supports_version: true,
            status: "installed",
            version: "3.14.7",
            error: null,
          },
        ]),
      )
      .mockResolvedValueOnce(
        Response.json({
          tenants: [
            {
              kind: "managed",
              name: "work",
              display_name: "work",
              home: "/aibox/tenants/work",
              exists: true,
              agents: [
                {
                  agent: "codex",
                  current_config: { present_files: 0, expected_files: 2, error: null },
                  named_configs: { entries: [], error: null },
                  application: { last_application: null, drift: "untracked", detail: null },
                },
              ],
              components: { entries: [], error: null },
            },
          ],
        }),
      );

    await expect(api.configs.revealConfigFile(target)).resolves.toMatchObject({
      visual_options: [{ value_kind: "string", value: "gpt-5", proxy_routed: false }],
      custom_provider: undefined,
      visual_error: undefined,
    });
    await expect(api.tenants.listComponents(tenant)).resolves.toEqual([
      expect.objectContaining({ kind: "python", status: "installed" }),
    ]);
    await expect(api.overview.loadTopology()).resolves.toEqual({
      tenants: [
        expect.objectContaining({
          kind: "managed",
          name: "work",
          agents: [
            expect.objectContaining({
              current_config: { present_files: 0, expected_files: 2 },
              named_configs: { entries: [] },
            }),
          ],
          components: { entries: [] },
        }),
      ],
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
    expect(fetchMock).toHaveBeenCalledWith(
      controlRoute("sessions_detail", {}, "tenant=managed%3Awork&agent=codex&id=session-1"),
      expect.objectContaining({ signal: undefined }),
    );
  });

  it("normalizes synchronous and asynchronous Component mutations", async () => {
    const { api, fetchMock } = connected();
    fetchMock
      .mockImplementationOnce(() =>
        Promise.resolve(
          Response.json({
            id: "operation-1",
            kind: "install codex",
            state: "running",
            started_at: "now",
            ended_at: null,
            result: null,
            first_sequence: 0,
            next_sequence: 0,
            logs: [],
          }),
        ),
      )
      .mockImplementationOnce(() => Promise.resolve(Response.json({ installed: "codex" })));

    await expect(api.tenants.mutateComponent(tenant, "codex", true, null)).resolves.toMatchObject({
      kind: "operation",
      operation: { id: "operation-1" },
    });
    await expect(
      api.tenants.mutateComponent(tenant, "codex-statusline", true, null),
    ).resolves.toEqual({
      kind: "completed",
      value: { installed: "codex" },
    });
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
