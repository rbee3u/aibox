import { expect, test } from "@playwright/test";

const sessionId = "01a01e0d-ee3f-71e3-8cd7-e62a5aff6b80";

test.beforeEach(async ({ page }) => {
  await page.route("**/_aibox/api/**", (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (url.pathname === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null } });
    }
    if (url.pathname === "/_aibox/api/operations/events") {
      return route.fulfill({ contentType: "text/event-stream", body: "" });
    }
    if (url.pathname === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "host",
            name: null,
            display_name: "Host Tenant",
            home: "/Users/example",
            exists: true,
          },
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/aibox/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (url.pathname === "/_aibox/api/sessions") {
      return route.fulfill({
        json: {
          sessions: [
            {
              id: sessionId,
              display_id: "01a01e0dee3f",
              start_ts: "2026-08-20T15:23:44Z",
              title: "Design a simpler Sessions detail experience",
              latest_message: "Implemented a focused conversation reader with compact diagnostics.",
              message_count: 4,
              tool_count: 2,
              warnings: ["encountered 2 unsupported Transcript Entry projection(s)"],
            },
            {
              id: "02b02f1e-ff4f-72f4-9de8-f73b6bgg7c91",
              display_id: "02b02f1eff4f",
              start_ts: "2026-08-20T14:18:23Z",
              title: "Review Request Proxy tests",
              latest_message:
                "Kept routine tests socket-free and improved deterministic synchronization.",
              message_count: 18,
              tool_count: 7,
              warnings: [],
            },
            {
              id: "03c03g2f-aa5a-73a5-aef9-a84c7chh8d02",
              display_id: "03c03g2faa5a",
              start_ts: "2026-08-19T11:53:55Z",
              title: "Improve unit tests for long-lived streams",
              latest_message:
                "Covered completion, partial reads, and malformed Transcript entries.",
              message_count: 32,
              tool_count: 14,
              warnings: [],
            },
          ],
          warnings: [],
          partial: false,
        },
      });
    }
    if (url.pathname === "/_aibox/api/sessions/detail") {
      const frames = [
        {
          type: "meta",
          meta: {
            id: sessionId,
            title: "Design a simpler Sessions detail experience",
            start_ts: "2026-08-20T15:23:44Z",
            transcript_path: `.codex/sessions/2026/08/20/rollout-${sessionId}.jsonl`,
            cwd: "/workspace/aibox",
            model_provider: "openai",
            cli_version: "0.148.0",
          },
        },
        {
          type: "message",
          message: {
            entry_ids: ["line-1"],
            role: "user",
            timestamp: "2026-08-20T15:23:44Z",
            text: "Please redesign the Sessions detail so the conversation is the primary content.",
          },
        },
        {
          type: "evidence",
          evidence: {
            entry_id: "line-2",
            line: 2,
            timestamp: "2026-08-20T15:23:44Z",
            native_type: "response_item",
            role: null,
            content_types: [],
            status: "unsupported",
            preview: "Unsupported provider record",
          },
        },
        {
          type: "evidence",
          evidence: {
            entry_id: "line-3",
            line: 3,
            timestamp: "2026-08-20T15:23:44Z",
            native_type: "world_state",
            role: null,
            content_types: [],
            status: "filtered",
            preview: "Filtered environment state",
          },
        },
        {
          type: "message",
          message: {
            entry_ids: ["line-4"],
            role: "assistant",
            timestamp: "2026-08-20T15:23:50Z",
            text: "## Direction\n\nThe detail now prioritizes **reading the conversation**.\n\n- Compact header and tabs\n- Diagnostics moved to Details\n- Safe Markdown and code blocks\n\n```ts\nconst view = 'conversation';\n```",
          },
        },
        {
          type: "tool_activity",
          tool_activity: {
            entry_ids: ["line-5"],
            call_id: "call-1",
            timestamp: "2026-08-20T15:23:52Z",
            name: "apply_patch",
            status: "completed",
            summary: "Updated the Sessions React view and CSS module.",
          },
        },
        {
          type: "message",
          message: {
            entry_ids: ["line-6"],
            role: "user",
            timestamp: "2026-08-20T15:23:55Z",
            text: "The activity should stay secondary to the conversation.",
          },
        },
        {
          type: "message",
          message: {
            entry_ids: ["line-7"],
            role: "assistant",
            timestamp: "2026-08-20T15:23:57Z",
            text: "The destructive action remains in the catalog, while the detail is now a focused reader.",
          },
        },
        {
          type: "complete",
          stats: {
            start_ts: "2026-08-20T15:23:44Z",
            last_event_ts: "2026-08-20T15:23:57Z",
            observed_duration_ms: 13000,
            message_count: 5,
            tool_count: 2,
            entry_count: 7,
            malformed_count: 0,
            unsupported_count: 2,
            hidden_internal_count: 0,
            file_size: 13812,
            snapshot: "13812:1",
          },
          warnings: ["encountered 2 unsupported Transcript Entry projection(s)"],
        },
      ];
      return route.fulfill({
        contentType: "application/x-ndjson",
        body: `${frames.map((frame) => JSON.stringify(frame)).join("\n")}\n`,
      });
    }
    return route.fulfill({ status: 404, json: { error: `Unexpected ${url.pathname}` } });
  });
});

test("capture redesigned Sessions detail", async ({ page }) => {
  await page.setViewportSize({ width: 1512, height: 900 });
  await page.goto(
    `/_aibox/ui/sessions?tenant=managed%3Adefault&agent=codex&session_tenant=managed%3Adefault&session_agent=codex&session=${sessionId}`,
  );
  await page.getByRole("heading", { name: "Direction" }).waitFor();
  await expect(page.getByRole("button", { name: /Jump to message/ })).toHaveCount(2);
  await expect(page.getByText("Agent activity")).toHaveCount(1);
  await page.screenshot({ path: "/tmp/aibox-sessions-desktop.png", fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await page.getByRole("heading", { name: "Direction" }).waitFor();
  await expect(page.getByRole("button", { name: /Jump to message/ })).toHaveCount(2);
  await page.screenshot({ path: "/tmp/aibox-sessions-mobile.png", fullPage: true });
});
