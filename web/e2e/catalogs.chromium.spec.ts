import { expect, test, type Page } from "@playwright/test";
import { catalogSession, mockCatalogs } from "./catalogs.fixture";

async function expectNoPageOverflow(page: Page) {
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
    .toBe(true);
}

for (const colorScheme of ["light", "dark"] as const) {
  test(`Tenant catalog, components, and dialogs preserve layout in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await mockCatalogs(page);
    await page.goto("../tenants?tenant=managed%3Awork");
    const copy = page.getByRole("button", { name: "Copy Tenant Home for work", exact: true });
    for (const width of [1280, 850]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(copy).toBeVisible();
      await expect(page.getByText("/var/lib/aibox/tenants/work", { exact: true })).toBeVisible();
      const copyBox = await copy.boundingBox();
      const deleteBox = await page
        .getByRole("button", { name: "Delete Tenant work", exact: true })
        .boundingBox();
      expect(copyBox).not.toBeNull();
      expect(deleteBox).not.toBeNull();
      expect(copyBox!.width).toBe(copyBox!.height);
      expect(copyBox!.height).toBe(deleteBox!.height);
      await expectNoPageOverflow(page);
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByRole("button", { name: "Back to Tenants", exact: true })).toBeVisible();
    await expectNoPageOverflow(page);
    await page.getByRole("button", { name: "Back to Tenants", exact: true }).click();
    await expect(copy).toBeVisible();
    await page.getByRole("button", { name: "Select Tenants", exact: true }).click();
    const select = page.getByRole("button", { name: "Select work", exact: true });
    await select.click();
    await expect(page.getByRole("button", { name: "Deselect work", exact: true })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect(page.locator('[data-selected="true"]')).toHaveCount(1);
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.getByRole("button", { name: "Create Managed Tenant", exact: true }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Tenant name", exact: true })).toBeVisible();
    await expectNoPageOverflow(page);
  });

  test(`Session catalog, conversation, details, and selection preserve layout in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await mockCatalogs(page);
    await page.goto(`../sessions?tenant=managed%3Awork&agent=codex&session=${catalogSession.id}`);
    for (const width of [1280, 850, 390]) {
      await page.setViewportSize({ width, height: 844 });
      const back = page.getByRole("button", { name: "Back to Sessions", exact: true });
      if (width <= 760) await expect(back).toBeVisible();
      else await expect(back).toBeHidden();
      await expect(
        page
          .getByRole("article")
          .getByText("Explain the architecture of this project.", { exact: true }),
      ).toBeVisible();
      await expectNoPageOverflow(page);
      await page.getByRole("button", { name: "Details", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Copy Session ID", exact: true }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Copy Transcript path", exact: true }),
      ).toBeVisible();
      await expectNoPageOverflow(page);
      await page.getByRole("button", { name: "Conversation", exact: true }).click();
    }
    await page.getByRole("button", { name: "Back to Sessions", exact: true }).click();
    await page.getByRole("button", { name: "Select Sessions", exact: true }).click();
    const select = page.getByRole("button", {
      name: `Select ${catalogSession.title}`,
      exact: true,
    });
    await select.click();
    await expect(
      page.getByRole("button", { name: `Deselect ${catalogSession.title}`, exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator('[data-selected="true"]')).toHaveCount(1);
    await expectNoPageOverflow(page);
  });
}
