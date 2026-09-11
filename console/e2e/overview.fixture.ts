import type { Page } from "@playwright/test";
import type { OverviewData, TopologyData, TopologyTenant } from "../src/api/overview";

const tenant: TopologyTenant = {
  kind: "managed",
  name: "default",
  display_name: "default",
  exists: true,
  home: "/tenants/default",
  agents: [
    {
      agent: "codex",
      current_config: { present_files: 2, expected_files: 2 },
      application: {
        last_application: { applied: "personal", applied_at: "2026-09-09T00:00:00Z" },
        drift: "clean",
      },
      named_configs: { count: 2, attention: [] },
      sessions: { count: 12 },
    },
    {
      agent: "claude",
      current_config: { present_files: 1, expected_files: 1 },
      application: { last_application: null, drift: "untracked" },
      named_configs: { count: 0, attention: [] },
      sessions: { count: 0 },
    },
  ],
  components: { total: 8, installed: 8, attention: [] },
};
export const longTenantName =
  "workspace-with-a-very-long-tenant-name-that-must-wrap-without-hiding-controls";
export async function mockOverview(page: Page, errors = false) {
  const requests: string[] = [];
  const overview: OverviewData = {
    service: {
      version: "test",
      listen: "127.0.0.1:4173",
      uptime_seconds: 90,
      aibox_root: "/long/path/".repeat(12),
    },
    docker: { status: "available", error: null },
    runtime_image: {
      reference: "aibox:latest",
      status: "built",
      id: "sha256:abcdef1234567890",
      created_at: null,
      size_bytes: 4000000,
      detail: null,
    },
    managed_tenants: 3,
    host_available: true,
    host_home: "/home/test",
  };
  const data: TopologyData = {
    tenants: [
      { ...tenant, kind: "host", name: null, display_name: "Host Tenant" },
      tenant,
      {
        ...tenant,
        name: "shadow1",
        display_name: "shadow1",
        agents: [
          {
            ...tenant.agents[0],
            application: { last_application: null, drift: "dirty" },
            named_configs: {
              count: 0,
              attention: [],
              ...(errors ? { error: "Catalog could not be read" } : {}),
            },
          },
        ],
      },
      {
        ...tenant,
        name: longTenantName,
        display_name: longTenantName,
        agents: [
          {
            ...tenant.agents[0],
            application: {
              last_application: {
                applied: "a-very-long-config-name-".repeat(8),
                applied_at: "2026-09-09T00:00:00Z",
              },
              drift: "clean",
            },
          },
        ],
      },
    ],
  };
  await page.route("**/_aibox/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    requests.push(path);
    if (path.endsWith("/bootstrap"))
      return route.fulfill({
        json: { version: "test", csrf_token: "test", listen: "127.0.0.1:4173" },
      });
    if (path.endsWith("/operations/current"))
      return route.fulfill({ json: { operation: null, gap: false } });
    if (path.endsWith("/operations/events"))
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    if (path.endsWith("/overview")) return route.fulfill({ json: overview });
    if (path.endsWith("/topology")) return route.fulfill({ json: data });
    throw new Error("Unexpected Overview request: " + path);
  });
  return requests;
}
