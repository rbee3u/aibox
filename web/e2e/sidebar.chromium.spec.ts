import { expect, test, type Page } from "@playwright/test";
import { mockOverview } from "./overview.fixture";

async function expectPrimaryNavigationGeometry(page: Page) {
  const navigation = page.getByRole("navigation", { name: "Modules" });
  const links = navigation.getByRole("link");
  await expect(navigation).toBeVisible();
  await expect(links).toHaveCount(5);
  await expect
    .poll(async () => {
      const geometry = await links.evaluateAll((elements) =>
        elements.map((element) => {
          const bounds = element.getBoundingClientRect();
          return {
            bottom: bounds.bottom,
            height: bounds.height,
            left: bounds.left,
            right: bounds.right,
          };
        }),
      );
      const navigationBounds = await navigation.evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return { left: bounds.left, right: bounds.right };
      });
      const heights = geometry.map(({ height }) => height);
      const gaps = geometry
        .slice(1)
        .map(({ bottom }, index) => bottom - geometry[index].bottom - geometry[index].height);
      const leftInset = geometry[0].left - navigationBounds.left;
      const rightInset = navigationBounds.right - geometry[0].right;
      return {
        count: geometry.length,
        balancedInsets: leftInset === rightInset,
        focusRoom: Math.min(leftInset, rightInset) >= 4,
        separatedTargets: gaps.every((gap) => gap > 0 && gap < heights[0]),
        targetSize: heights.every((height) => height >= 44),
        uniformGaps: new Set(gaps).size === 1,
        uniformHeights: new Set(heights).size === 1,
      };
    })
    .toEqual({
      balancedInsets: true,
      count: 5,
      focusRoom: true,
      separatedTargets: true,
      targetSize: true,
      uniformGaps: true,
      uniformHeights: true,
    });

  const first = links.first();
  await first.focus();
  await first.press("Tab");
  await page.keyboard.press("Shift+Tab");
  const focus = await first.evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    const sidebarBounds = element.closest("aside")!.getBoundingClientRect();
    const style = getComputedStyle(element);
    const reach = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
    return {
      clipped:
        bounds.left - reach < sidebarBounds.left || bounds.right + reach > sidebarBounds.right,
      outlineStyle: style.outlineStyle,
      outlineWidth: parseFloat(style.outlineWidth),
    };
  });
  expect(focus.outlineStyle).not.toBe("none");
  expect(focus.outlineWidth).toBeGreaterThanOrEqual(2);
  expect(focus.clipped).toBe(false);
}

for (const colorScheme of ["light", "dark"] as const) {
  test(`primary navigation keeps its geometry in ${colorScheme} shell layouts`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 1512, height: 830 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await mockOverview(page);
    await page.goto("../overview");
    await expect(page.getByRole("table", { name: "Tenant status", exact: true })).toBeVisible();

    await expectPrimaryNavigationGeometry(page);
    await page.screenshot({ path: testInfo.outputPath("sidebar-expanded.png") });

    await page.getByRole("button", { name: "Collapse sidebar" }).click();
    await expectPrimaryNavigationGeometry(page);

    await page.setViewportSize({ width: 760, height: 720 });
    await page.getByRole("button", { name: "Open navigation" }).click();
    await expectPrimaryNavigationGeometry(page);

    await page.setViewportSize({ width: 812, height: 375 });
    await expectPrimaryNavigationGeometry(page);
    const sidebar = page.getByLabel("Console navigation", { exact: true });
    await expect
      .poll(() => sidebar.evaluate((element) => getComputedStyle(element).overflowY))
      .toBe("auto");
    await expect
      .poll(() =>
        page
          .getByRole("navigation", { name: "Modules" })
          .evaluate((element) => getComputedStyle(element).overflowY),
      )
      .toBe("visible");
  });
}
