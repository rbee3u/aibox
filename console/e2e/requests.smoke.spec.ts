import { expect, test } from "@playwright/test";
import { mockRequests } from "./requests.fixture";

test("desktop layout and keyboard interactions", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockRequests(page);
  await page.goto("./");

  await page.getByRole("button", { name: "Color theme: System" }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.getByRole("separator")).toHaveCount(0);
  const recordList = page.getByRole("complementary", { name: "Request Record list" });
  const listBounds = await recordList.boundingBox();
  expect(listBounds?.width).toBeGreaterThan(390);
  expect(listBounds?.width).toBeLessThan(405);

  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(page.getByRole("region", { name: "Request Record details" })).toBeVisible();

  await page.setViewportSize({ width: 760, height: 720 });
  await expect(recordList).toBeHidden();
  await expect(page.getByRole("button", { name: "Back to Request Record list" })).toBeVisible();
  await page.getByRole("button", { name: "Back to Request Record list" }).click();
  await expect(recordList).toBeVisible();

  await page.setViewportSize({ width: 761, height: 720 });
  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(recordList).toBeVisible();
  await expect(page.getByRole("region", { name: "Request Record details" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Back to Request Record list" })).toBeHidden();

  await page.getByRole("button", { name: "Select" }).click();
  await page.getByRole("button", { name: "Select POST relay.example.test/v1/responses" }).click();
  await page.getByRole("button", { name: "Delete selected" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: "Delete selected" })).toBeFocused();
});
