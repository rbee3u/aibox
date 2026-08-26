import { expect, test } from "@playwright/test";
import { mockOverview } from "./overview.fixture";

test("Overview exposes the complete resource map before Session discovery", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const { sessionRequests } = await mockOverview(page);
  await page.goto("../overview");

  const status = page.getByRole("region", { name: "Service status" });
  await expect(status.getByText("Managed Tenants")).toBeVisible();
  await expect(status.getByText("Console-only view of the Host Home")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Key facts" })).toBeVisible();

  const tree = page.getByRole("tree", { name: "Tenant resource topology" });
  await expect(tree).toBeVisible();
  await expect(tree.getByText("Host Tenant")).toBeVisible();
  await expect(tree.getByRole("link", { name: /^default / })).toBeVisible();
  await expect(tree.getByText("Current Config").first()).toBeVisible();
  await expect(tree.getByText("daily").first()).toBeVisible();
  await expect(tree.getByText("Rust")).toBeVisible();
  expect(sessionRequests).toHaveLength(0);

  await page.getByRole("button", { name: "Expand Sessions" }).first().click();
  await expect(tree.getByText("3 Sessions").first()).toBeVisible();
  expect(sessionRequests).toHaveLength(1);
});

test("390px Overview keeps page overflow contained inside the topology viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockOverview(page);
  await page.goto("../overview");

  await expect(page.getByRole("heading", { name: "Resource topology" })).toBeVisible();
  await expect(page.getByRole("tree", { name: "Tenant resource topology" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Fit topology to width" })).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);

  const topologyViewport = page.locator("[data-topology-viewport]");
  await expect
    .poll(() => topologyViewport.evaluate((element) => element.scrollWidth > element.clientWidth))
    .toBe(true);
});

test("operational surfaces keep the compact density contract across target widths", async ({
  page,
}) => {
  await mockOverview(page);
  for (const width of [1280, 1440, 1600, 1920, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("../overview");

    const status = page.getByRole("region", { name: "Service status" });
    const facts = status.locator("[data-overview-fact]");
    await expect(facts).toHaveCount(6);
    await expect
      .poll(() =>
        facts.evaluateAll((elements) =>
          elements.every((element) => element.getBoundingClientRect().height > 0),
        ),
      )
      .toBe(true);
    const factHeights = await facts.evaluateAll((elements) =>
      elements.map((element) => Math.round(element.getBoundingClientRect().height)),
    );
    expect(factHeights).toHaveLength(6);
    expect(Math.max(...factHeights)).toBeLessThanOrEqual(80);
    expect(Math.min(...factHeights)).toBeGreaterThanOrEqual(60);
    await expect(status.getByText("Service", { exact: true })).toBeVisible();

    const toolbarHeight = await page
      .locator("[data-overview-toolbar]")
      .evaluate((element) => Math.round(element.getBoundingClientRect().height));
    expect(toolbarHeight).toBeLessThanOrEqual(48);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true);
    await page.screenshot({ path: `/tmp/aibox-overview-${width}.png`, fullPage: true });
  }
});
