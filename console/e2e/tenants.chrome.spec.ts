import { expect, test, type Locator, type Page } from "@playwright/test";
import type { ComponentRow, TenantRow } from "../src/controlApi";

const tenants = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/home/test/.aibox/tenants/default",
    exists: true,
  },
] satisfies TenantRow[];

const components = [
  component("codex", true, "not-installed"),
  component("codex-statusline", false, "installed"),
  component("claude", true, "not-installed"),
  component("claude-statusline", false, "installed"),
  component("node", true, "not-installed"),
  component("python", true, "not-installed"),
  component("rust", true, "installed", "1.97.1"),
  component("go", true, "installed", "1.26.5"),
] satisfies ComponentRow[];

test("Component names stay left-aligned across labels and action sets", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => localStorage.setItem("aibox-console-theme", "light"));
  await mockTenants(page);
  await page.goto("../tenants?tenant=managed%3Adefault");

  const codex = componentRow(page, "Codex CLI");
  const codexStatusline = componentRow(page, "Codex status line");
  const claudeStatusline = componentRow(page, "Claude status line");
  const rust = componentRow(page, "Rust toolchain");
  const go = componentRow(page, "Go toolchain");

  const rows = [codex, codexStatusline, claudeStatusline, rust, go];
  const iconOffsets = await Promise.all(
    rows.map(async (row) => (await box(row.locator("svg").first())).x),
  );
  const nameOffsets = await Promise.all(
    rows.map(async (row) => (await box(row.locator("strong"))).x),
  );

  expect(new Set(iconOffsets.map(Math.round)).size).toBe(1);
  expect(new Set(nameOffsets.map(Math.round)).size).toBe(1);
});

function component(
  kind: ComponentRow["kind"],
  supportsVersion: boolean,
  status: ComponentRow["status"],
  version: string | null = null,
): ComponentRow {
  return {
    kind,
    supports_version: supportsVersion,
    status,
    version,
    error: null,
  };
}

function componentRow(page: Page, label: string): Locator {
  return page.getByRole("button", { name: new RegExp(`^${label}`) }).locator("..");
}

async function box(locator: Locator) {
  await expect(locator).toBeVisible();
  const bounds = await locator.boundingBox();
  expect(bounds).not.toBeNull();
  return bounds!;
}

async function mockTenants(page: Page) {
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
    if (url.pathname === "/_aibox/api/tenants") return route.fulfill({ json: tenants });
    if (url.pathname === "/_aibox/api/components") return route.fulfill({ json: components });
    throw new Error(`Unexpected Tenants API request: ${request.method()} ${url.pathname}`);
  });
}
