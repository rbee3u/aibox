import type { Page } from "@playwright/test";
import type { TenantRow } from "../src/api/core";
import type { ComponentKind, ComponentRow } from "../src/api/tenants";
import type { SessionDetailFrame, SessionRow } from "../src/api/sessions";

export const catalogTenantRows: TenantRow[] = [
  { kind: "host", name: null, display_name: "Host Tenant", home: "/home/test", exists: true },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/home/test/.aibox/tenants/default",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/var/lib/aibox/tenants/work",
    exists: true,
  },
];
export const catalogSession: SessionRow = {
  id: "11111111-1111-1111-1111-111111111111",
  display_id: "111111111111",
  start_ts: "2026-08-17T09:00:00Z",
  title: "Inspect a saved conversation",
  latest_message: "A readable assistant response",
  message_count: 2,
  tool_count: 0,
  warnings: [],
};

/** Native Tenant and Session fixtures shared by real-layout catalog scenarios. */
export async function mockCatalogs(page: Page) {
  const kinds: ComponentKind[] = [
    "codex",
    "codex-statusline",
    "claude",
    "claude-statusline",
    "node",
    "python",
    "rust",
    "go",
  ];
  const components: ComponentRow[] = kinds.map((kind) => ({
    kind,
    supports_version: !kind.endsWith("statusline"),
    status: "installed",
    version: kind.endsWith("statusline") ? null : "1.0.0",
    error: null,
  }));
  const frames: SessionDetailFrame[] = [
    {
      type: "meta",
      meta: {
        id: catalogSession.id,
        title: catalogSession.title,
        start_ts: catalogSession.start_ts,
        transcript_path: "/tenants/work/.codex/sessions/rollout.jsonl",
        cwd: "/workspace/project",
        model_provider: "openai",
        cli_version: "test",
      },
    },
    {
      type: "message",
      message: {
        entry_ids: ["user-1"],
        role: "user",
        timestamp: catalogSession.start_ts,
        text: "Explain the architecture of this project.",
      },
    },
    {
      type: "message",
      message: {
        entry_ids: ["assistant-1"],
        role: "assistant",
        timestamp: catalogSession.start_ts,
        text: "# Response\nThe Service composes domain capabilities and the Console reads their projections.",
      },
    },
    {
      type: "complete",
      warnings: [],
      stats: {
        start_ts: catalogSession.start_ts,
        last_event_ts: catalogSession.start_ts,
        observed_duration_ms: 0,
        message_count: 2,
        tool_count: 0,
        entry_count: 2,
        malformed_count: 0,
        unsupported_count: 0,
        hidden_internal_count: 0,
        file_size: 200,
        snapshot: "200:1",
      },
    },
  ];
  await page.route("**/_aibox/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/bootstrap"))
      return route.fulfill({
        json: { version: "test", csrf_token: "test", listen: "127.0.0.1:4173" },
      });
    if (path.endsWith("/operations/current"))
      return route.fulfill({ json: { operation: null, gap: false } });
    if (path.endsWith("/operations/events"))
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null,"gap":false}\n\n',
      });
    if (path.endsWith("/tenants") && route.request().method() === "GET")
      return route.fulfill({ json: catalogTenantRows });
    if (path.endsWith("/components/latest")) return route.fulfill({ json: null });
    if (path.endsWith("/components")) return route.fulfill({ json: components });
    if (path.endsWith("/sessions/detail"))
      return route.fulfill({
        contentType: "application/x-ndjson",
        body: frames.map((frame) => JSON.stringify(frame)).join("\n") + "\n",
      });
    if (path.endsWith("/sessions"))
      return route.fulfill({ json: { sessions: [catalogSession], warnings: [], partial: false } });
    throw new Error(`Unexpected catalog request: ${route.request().method()} ${path}`);
  });
}
