import type { Page } from "@playwright/test";
import type { OverviewData, TopologyAgent, TopologyData } from "../src/controlApi";

const overview = {
  service: {
    version: "test",
    listen: "127.0.0.1:9923",
    uptime_seconds: 120,
    aibox_root: "/var/lib/aibox",
  },
  docker: { status: "available", error: null },
  runtime_image: {
    reference: "aibox:test",
    status: "built",
    id: "sha256:0123456789abcdef",
    created_at: "2026-08-19T01:00:00Z",
    size_bytes: 4_194_304,
    detail: null,
  },
  managed_tenants: 1,
  host_available: true,
  requests: { total: 7, active: 1, warning: 1, error: 0, bytes: 4096 },
} satisfies OverviewData;

const agent = {
  agent: "codex",
  current_config: { present_files: 2, expected_files: 2 },
  named_configs: {
    entries: [
      { name: "daily", state: "ready" },
      { name: "repair-me", state: "incomplete", detail: "auth.json is missing" },
    ],
  },
  application: {
    last_application: { applied: "daily", applied_at: "2026-08-19T02:00:00Z" },
    drift: "dirty",
  },
} satisfies TopologyAgent;

const topology = {
  tenants: [
    {
      kind: "managed",
      name: "default",
      display_name: "default",
      home: "/var/lib/aibox/tenants/default",
      exists: true,
      agents: [agent],
      components: {
        entries: [
          {
            kind: "rust",
            supports_version: true,
            status: "installed",
            version: "1.89.0",
            error: null,
          },
        ],
      },
    },
    {
      kind: "host",
      name: null,
      display_name: "Host Tenant",
      home: "/home/test",
      exists: true,
      agents: [
        { ...agent, named_configs: { entries: [{ name: "host-default", state: "ready" }] } },
      ],
      components: { entries: [] },
    },
  ],
} satisfies TopologyData;

export async function mockOverview(page: Page) {
  const sessionRequests: string[] = [];
  await page.route("**/_aibox/api/**", (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (url.pathname === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (url.pathname === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (url.pathname === "/_aibox/api/overview") return route.fulfill({ json: overview });
    if (url.pathname === "/_aibox/api/topology") return route.fulfill({ json: topology });
    if (url.pathname === "/_aibox/api/sessions/summary") {
      sessionRequests.push(url.search);
      return route.fulfill({ json: { count: 3, warnings: [], partial: false } });
    }
    throw new Error(`Unexpected Overview API request: ${request.method()} ${url.pathname}`);
  });
  return { sessionRequests };
}
