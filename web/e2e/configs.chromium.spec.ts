import { expect, test } from "@playwright/test";
import { mockConfigWorkflows } from "./configs.fixture";

for (const colorScheme of ["light", "dark"] as const) {
  test(`catalog action labels adapt without overflow in ${colorScheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await mockConfigWorkflows(page);
    await page.route("**/_aibox/api/tenants", async (route) => {
      await route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "A very long tenant display name for toolbar wrapping",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    });
    await page.goto("../configs?tenant=managed%3Adefault&agent=claude&named=1");
    const refresh = page.getByRole("button", { name: "Refresh Configs", exact: true });
    const select = page.getByRole("button", { name: "Select Configs", exact: true });
    for (const width of [1280, 761, 760, 390]) {
      await page.setViewportSize({ width, height: 844 });
      for (const [button, label] of [
        [refresh, "Refresh"],
        [select, "Select"],
      ] as const) {
        await expect(button).toBeVisible();
        if (width > 760) await expect(button.getByText(label, { exact: true })).toBeVisible();
        else {
          await expect(button.getByText(label, { exact: true })).toBeHidden();
          const bounds = await button.boundingBox();
          expect(bounds!.width).toBe(bounds!.height);
        }
      }
      await expect
        .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), {
          message: `No page overflow at ${width}px`,
        })
        .toBe(true);
    }
    await select.click();
    await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeVisible();
  });
}

test("keeps the dual-filter catalog toolbar on one row at desktop width", async ({ page }) => {
  await mockConfigWorkflows(page);
  await page.goto("../configs?tenant=managed%3Adefault&agent=claude&named=1");
  await page.setViewportSize({ width: 1024, height: 844 });

  const toolbar = page.locator('[class*="toolbar"]').first();
  const filters = page.locator('[class*="toolbarFilters"]').first();
  const actions = page.locator('[class*="toolbarActions"]').first();
  await expect(toolbar).toBeVisible();
  await expect(filters).toBeVisible();
  await expect(actions).toBeVisible();
  const toolbarBox = await toolbar.boundingBox();
  const filtersBox = await filters.boundingBox();
  const actionsBox = await actions.boundingBox();
  expect(toolbarBox).not.toBeNull();
  expect(filtersBox).not.toBeNull();
  expect(actionsBox).not.toBeNull();
  expect(Math.abs(filtersBox!.y - actionsBox!.y)).toBeLessThanOrEqual(1);
  expect(filtersBox!.width).toBeGreaterThanOrEqual(248);
  expect(filtersBox!.width).toBeLessThanOrEqual(308);
  expect(filtersBox!.y).toBeGreaterThanOrEqual(toolbarBox!.y);
  expect(filtersBox!.y + filtersBox!.height).toBeLessThanOrEqual(
    toolbarBox!.y + toolbarBox!.height,
  );
});

test("compact catalog actions retain coarse-pointer targets", async ({ browser }) => {
  const context = await browser.newContext({
    hasTouch: true,
    viewport: { width: 390, height: 844 },
  });
  const page = await context.newPage();
  await mockConfigWorkflows(page);
  await page.goto("http://127.0.0.1:4173/_aibox/ui/configs?named=1");
  for (const name of ["Refresh Configs", "Select Configs"]) {
    const button = page.getByRole("button", { name, exact: true });
    await expect(button).toBeVisible();
    const bounds = await button.boundingBox();
    expect(bounds!.width).toBeGreaterThanOrEqual(44);
    expect(bounds!.height).toBeGreaterThanOrEqual(44);
  }
  await context.close();
});

test("Config editing keeps Visual, Raw, Named, and Current modes explicit", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockConfigWorkflows(page);
  await page.goto(
    "../configs?tenant=managed%3Adefault&agent=claude&config=team&file=settings.json",
  );

  await expect(page.locator(".cm-editor")).toHaveCount(0);
  const contextBounds = await page.getByLabel("Config editing context").boundingBox();
  expect(contextBounds).not.toBeNull();
  await expect(page.getByRole("button", { name: "Visual" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.getByRole("button", { name: "Visual" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.getByLabel("Config editing context")).toContainText("team");
  await expect(page.getByRole("textbox", { name: "Base URL" })).toBeVisible();

  await page.getByRole("button", { name: "Raw" }).click();
  await expect(page.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".cm-editor")).toBeVisible();

  await page.getByRole("button", { name: "Back to Configs" }).click();
  const team = page.getByRole("button", { name: "team", exact: true });
  await expect(team).toBeVisible();
  await expect(team).toBeFocused();

  await page.goto("../configs?tenant=managed%3Adefault&agent=claude&current=1&file=settings.json");
  await expect(page.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Visual" })).toHaveCount(0);
  await expect(page.getByLabel("Config editing context")).toContainText("Current Config");
});

for (const colorScheme of ["light", "dark"] as const) {
  test(`Visual field captions do not activate controls in ${colorScheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await mockConfigWorkflows(page);
    await page.goto(
      "../configs?tenant=managed%3Adefault&agent=claude&config=team&file=settings.json",
    );
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 844 });
      const select = page.getByRole("combobox", { name: "Default permission mode value" });
      await page.getByText("Default permission mode", { exact: true }).click();
      await expect(select).not.toBeFocused();
      await expect(page.getByRole("listbox")).toHaveCount(0);
      await select.click();
      await expect(page.getByRole("listbox")).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(select).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(page.getByRole("listbox")).toBeVisible();
      await page.keyboard.press("Escape");
      await page.getByText("Base URL", { exact: true }).click();
      await expect(page.getByRole("textbox", { name: "Base URL" })).not.toBeFocused();
      await expect(page.getByRole("button", { name: /^Help for/ })).toHaveCount(0);
      await expect
        .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
        .toBe(true);
    }
  });
}

for (const colorScheme of ["light", "dark"] as const) {
  test(`Config differences remain navigable in Visual and Raw in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await mockConfigWorkflows(page);
    await page.goto(
      "../configs?tenant=managed%3Adefault&agent=claude&config=team&file=settings.json",
    );
    await expect(page.getByText("Differs from Current Config")).toBeVisible();
    await page.getByText("Differs from Current Config").click();
    await expect(page.getByText('"https://current.example.test"').last()).toBeVisible();
    await page.getByRole("button", { name: "Raw", exact: true }).click();
    const marker = page.getByRole("button", { name: "Show difference for env.ANTHROPIC_BASE_URL" });
    await expect(marker).toBeVisible();
    await expect(page.locator(".cm-config-difference-line")).toHaveCount(1);
    await marker.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByText('"https://current.example.test"').first()).toBeVisible();
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 844 });
      await expect
        .poll(
          async () => {
            const markerBox = await marker.boundingBox();
            const line = await page.locator(".cm-config-difference-line").evaluate((element) => ({
              top: element.getBoundingClientRect().top,
              lineHeight: Number.parseFloat(getComputedStyle(element).lineHeight),
            }));
            if (!markerBox) return Number.POSITIVE_INFINITY;
            // A wrapped line's marker belongs to its first visual row.
            return Math.max(
              Math.abs(markerBox.y - line.top),
              Math.abs(markerBox.height - line.lineHeight),
            );
          },
          { message: "Difference marker aligns with the first code row" },
        )
        .toBeLessThanOrEqual(1);
      await expect
        .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
        .toBe(true);
    }
  });
}
