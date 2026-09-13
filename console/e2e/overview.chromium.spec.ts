import { expect, test } from "@playwright/test";
import { longTenantName, mockOverview } from "./overview.fixture";

for (const colorScheme of ["light", "dark"] as const) {
  for (const viewport of [
    { width: 1280, height: 720 },
    { width: 375, height: 812 },
    { width: 812, height: 375 },
  ]) {
    test(`Overview reflows in ${colorScheme} at ${viewport.width}×${viewport.height}`, async ({
      page,
    }, testInfo) => {
      await page.setViewportSize(viewport);
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      const requests = await mockOverview(page, true);
      await page.goto("../overview");
      const table = page.getByRole("table", { name: "Tenant status", exact: true });
      await expect(table).toBeVisible();
      await page.screenshot({ path: testInfo.outputPath("overview.png") });
      await expect(table.getByText(longTenantName, { exact: true })).toBeVisible();
      await expect(table.getByText(/^Last applied: a-very-long-config-name-/)).toBeVisible();
      await expect(table.getByText("Catalog inspection failed").first()).toBeVisible();
      const summary = page.getByText("Environment details", { exact: true });
      await summary.focus();
      await page.keyboard.press("Enter");
      await expect(
        page.getByRole("button", { name: "Copy AIBox Root", exact: true }),
      ).toBeVisible();
      // Stress text sizing separately from the viewport; don't rely on ellipses.
      await page.addStyleTag({
        content: "[data-overview-scroll] { --text-sm: 28px; --text-xs: 24px; }",
      });
      const overflow = await page.locator("[data-overview-scroll]").evaluate((element) => ({
        page: element.scrollWidth > element.clientWidth + 1,
        root: document.documentElement.scrollWidth > window.innerWidth + 1,
      }));
      expect(overflow).toEqual({ page: false, root: false });
      await page.getByRole("button", { name: "Refresh Overview", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Refresh Overview", exact: true }),
      ).toBeEnabled();
      // Session counts ride on the topology read, so Overview still opens none.
      expect(requests.some((path) => path.includes("/sessions"))).toBe(false);
    });
  }
}
test("Tenant rows give every destination one focusable link with a visible ring", async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 600 });
  await mockOverview(page);
  await page.goto("../overview");
  const table = page.getByRole("table", { name: "Tenant status", exact: true });
  await expect(table).toBeVisible();
  const sessions = table.getByRole("link", { name: /^12 Sessions/ }).first();
  await sessions.focus();
  const focusStyle = await sessions.evaluate((element) => {
    const style = getComputedStyle(element);
    const box = element.getBoundingClientRect();
    return {
      outline: style.outlineStyle,
      width: parseFloat(style.outlineWidth),
      visible:
        box.top >= 0 &&
        box.bottom <= window.innerHeight &&
        box.left >= 0 &&
        box.right <= window.innerWidth,
    };
  });
  expect(focusStyle.outline).not.toBe("none");
  expect(focusStyle.width).toBeGreaterThanOrEqual(2);
  expect(focusStyle.visible).toBe(true);
});
