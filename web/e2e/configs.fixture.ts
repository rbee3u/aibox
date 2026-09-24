import type { Page } from "@playwright/test";
import type { ConfigVisualOption } from "../src/api/configs";

const settingsContent = JSON.stringify(
  { env: { ANTHROPIC_BASE_URL: "https://api.example.test" } },
  null,
  2,
);

const visualFields = [
  {
    path: "env.ANTHROPIC_BASE_URL",
    label: "Base URL",
    description: "Endpoint used by Claude.",
    visible_when: null,
    group: "Endpoint & credentials",
    value_kind: "string",
    enum_values: [],
    sensitive: false,
    required: false,
    request_proxy_route: false,
    included: true,
    value: "https://api.example.test",
    proxy_routed: false,
  },
  {
    path: "permissions.defaultMode",
    label: "Default permission mode",
    description: "Permission mode",
    group: "Permissions",
    visible_when: null,
    value_kind: "string",
    enum_values: ["default", "bypassPermissions"],
    sensitive: false,
    required: true,
    request_proxy_route: false,
    included: true,
    value: "bypassPermissions",
    proxy_routed: false,
  },
] satisfies ConfigVisualOption[];

export async function mockConfigWorkflows(page: Page) {
  await page.route("**/_aibox/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({
        json: { version: "test", csrf_token: "test-token", listen: "127.0.0.1:3000" },
      });
    }
    if (path === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (path === "/_aibox/api/configs" && request.method() === "GET") {
      return route.fulfill({
        json: {
          configs: [{ name: "team", state: "ready" }],
          files: ["settings.json"],
          application: {
            last_application: { applied: "team", applied_at: "2026-08-19T02:00:00Z" },
            drift: "dirty",
          },
          credential_propagation_available: false,
        },
      });
    }
    if (path === "/_aibox/api/configs/compare") {
      const body = request.postDataJSON() as { files: Array<{ content_base64: string }> };
      const content = atob(body.files[0].content_base64);
      return route.fulfill({
        json: {
          source: "team",
          incomplete: false,
          files: [
            {
              file: "settings.json",
              error: null,
              named: { revision: "revision", exists: true, content },
              current: { revision: "current", exists: true, content },
              differences: [
                {
                  path: ["env", "ANTHROPIC_BASE_URL"],
                  named_present: true,
                  current_present: true,
                  named_value: "https://api.example.test",
                  current_value: "https://current.example.test",
                  sensitive: false,
                  named_range: null,
                  current_range: null,
                },
              ],
            },
          ],
        },
      });
    }
    if (path === "/_aibox/api/configs/reveal") {
      return route.fulfill({
        json: {
          file: "settings.json",
          exists: true,
          revision: "settings-revision",
          content_base64: btoa(settingsContent),
          visual_options: visualFields,
        },
      });
    }
    if (path === "/_aibox/api/configs/diagnose") {
      return route.fulfill({ json: { diagnostics: [] } });
    }
    throw new Error("Unexpected Config workflow API request: " + request.method() + " " + path);
  });
}
