import { expect, test, type Locator, type Page } from "@playwright/test";
import type { TenantRow } from "../src/api/core";
import type { ComponentLatestSnapshot, ComponentRow } from "../src/api/tenants";

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
  component("codex-statusline", false, "modified"),
  component("claude", true, "not-installed"),
  component("claude-statusline", false, "not-installed"),
  component("node", true, "not-installed"),
  component("python", true, "not-installed"),
  component("rust", true, "installed", "1.97.1"),
  component("go", true, "installed", "1.26.5"),
] satisfies ComponentRow[];

const latest = {
  checked_at: "2026-08-25T08:00:00Z",
  entries: [
    latestEntry("codex", "0.149.1", "github.com/openai/codex"),
    latestEntry("claude", "2.1.245", "registry.npmjs.org/@anthropic-ai/claude-code"),
    latestEntry("node", "24.19.0", "nodejs.org"),
    latestEntry("python", "3.14.7", "github.com/astral-sh/python-build-standalone"),
    latestEntry("rust", "1.98.0", "static.rust-lang.org"),
    latestEntry("go", "1.26.4", "go.dev"),
  ],
} satisfies ComponentLatestSnapshot;

test("Component names stay left-aligned across labels and action sets", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => localStorage.setItem("aibox-console-theme", "light"));
  await mockTenants(page);
  await page.goto("../tenants?tenant=managed%3Adefault");

  const codex = componentRow(page, "Codex");
  const codexStatusline = componentRow(page, "Codex Statusline");
  const claudeStatusline = componentRow(page, "Claude Statusline");
  const rust = componentRow(page, "Rust");
  const go = componentRow(page, "Go");

  const rows = [codex, codexStatusline, claudeStatusline, rust, go];
  const iconOffsets = await Promise.all(
    rows.map(async (row) => (await box(row.locator("[data-component-icon]"))).x),
  );
  const nameOffsets = await Promise.all(
    rows.map(async (row) => (await box(row.locator("strong").first())).x),
  );

  // Names and icons share one leading edge no matter which actions a row has.
  expect(new Set(iconOffsets.map(Math.round)).size).toBe(1);
  expect(new Set(nameOffsets.map(Math.round)).size).toBe(1);
  const rowHeights = await Promise.all(rows.map(async (row) => (await box(row)).height));
  expect(new Set(rowHeights.map(Math.round)).size).toBe(1);
  const splitInstall = await box(
    codex.getByRole("button", { name: "Install", exact: true }).locator(".."),
  );
  const plainInstall = await box(
    claudeStatusline.getByRole("button", { name: "Install", exact: true }),
  );
  const splitUpdate = await box(
    rust.getByRole("button", { name: "Update", exact: true }).locator(".."),
  );
  const plainUpdate = await box(
    codexStatusline.getByRole("button", { name: "Update", exact: true }),
  );
  const updateRemove = await box(rust.getByRole("button", { name: "Remove Rust" }));
  const diagnosticRemove = await box(go.getByRole("button", { name: "Remove Go" }));
  // A split action occupies the same width as its plain counterpart, and the
  // icon-only Remove keeps one width wherever it appears.
  expect(Math.abs(updateRemove.width - diagnosticRemove.width)).toBeLessThan(1);
  expect(Math.abs(splitInstall.width - plainInstall.width)).toBeLessThan(1);
  expect(Math.abs(splitUpdate.width - plainUpdate.width)).toBeLessThan(1);
  const iconTile = await box(rust.locator("[data-component-icon]"));
  expect(updateRemove.x).toBeGreaterThan(splitUpdate.x + splitUpdate.width);
  expect(
    Math.abs(splitUpdate.y + splitUpdate.height / 2 - (updateRemove.y + updateRemove.height / 2)),
  ).toBeLessThan(1);
  expect(
    Math.abs(splitUpdate.y + splitUpdate.height / 2 - (iconTile.y + iconTile.height / 2)),
  ).toBeLessThan(1);
  await expect(codex.locator("button").filter({ hasText: "Codex" })).toHaveCount(0);
  await expect(codex).not.toHaveAttribute("aria-pressed");
  await expect(codex.locator("[title]")).toHaveCount(0);

  const installOptions = codex.getByRole("button", { name: "Install options for Codex" });
  const installOptionsBox = await box(installOptions);
  await installOptions.click();
  const installMenu = page.getByRole("menu", { name: "Codex install options" });
  const installMenuBox = await box(installMenu);
  expect(
    Math.abs(
      installMenuBox.x + installMenuBox.width - (installOptionsBox.x + installOptionsBox.width),
    ),
  ).toBeLessThan(1);
  expect(await installMenu.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
    true,
  );
  await page.keyboard.press("Escape");
  await expect(installOptions).toBeFocused();

  const updateOptions = rust.getByRole("button", { name: "Update options for Rust" });
  const updateOptionsBox = await box(updateOptions);
  await updateOptions.click();
  const updateMenu = page.getByRole("menu", { name: "Rust update options" });
  const updateMenuBox = await box(updateMenu);
  expect(
    Math.abs(updateMenuBox.x + updateMenuBox.width - (updateOptionsBox.x + updateOptionsBox.width)),
  ).toBeLessThan(1);
  expect(await updateMenu.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
    true,
  );
  await page.keyboard.press("Escape");
  await expect(updateOptions).toBeFocused();
  await page.screenshot({ path: "/tmp/aibox-tenants-1440.png", fullPage: true });
});

test("Component actions wrap without horizontal overflow on a narrow viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => localStorage.setItem("aibox-console-theme", "light"));
  await mockTenants(page);
  await page.goto("../tenants?tenant=managed%3Adefault&component=python");

  await expect(page).toHaveURL(/tenants\?tenant=managed%3Adefault$/);
  const catalog = page.locator('[aria-label="Components"]');
  await expect(catalog).toBeVisible();
  expect(await catalog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
    true,
  );
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await expect(page.getByText("2/8 installed", { exact: true })).toBeHidden();
  await expect(page.getByText(/Checked /)).toBeHidden();
  await expect(page.getByText("2 issues", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy Tenant Home" })).toBeHidden();
  await expect(page.getByRole("button", { name: "Check for updates" })).toBeVisible();

  const codex = componentRow(page, "Codex");
  const installOptions = codex.getByRole("button", { name: "Install options for Codex" });
  await installOptions.click();
  await expect(page.getByRole("menuitem", { name: "Install version…" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(installOptions).toBeFocused();

  const go = componentRow(page, "Go");
  await go.getByRole("button", { name: "Details" }).click();
  await expect(go).toContainText("observed release is lower");
  await page.screenshot({ path: "/tmp/aibox-tenants-390.png", fullPage: true });
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

function latestEntry(
  kind: ComponentRow["kind"],
  version: string,
  source: string,
): ComponentLatestSnapshot["entries"][number] {
  return { kind, state: "available", version, source, error: null };
}

function componentRow(page: Page, label: string): Locator {
  return page.getByRole("listitem").filter({ has: page.getByText(label, { exact: true }) });
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
    if (url.pathname === "/_aibox/api/components/latest") {
      return route.fulfill({ json: latest });
    }
    if (url.pathname === "/_aibox/api/components/latest/check") {
      return route.fulfill({ json: latest });
    }
    throw new Error(`Unexpected Tenants API request: ${request.method()} ${url.pathname}`);
  });
}
