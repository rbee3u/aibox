import { expect, test } from "@playwright/test";
import { mockTraffic } from "./traffic.fixture";

test("desktop layout and keyboard interactions", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockTraffic(page);
  await page.goto("./");

  await page.getByRole("combobox", { name: "Color theme" }).selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  const splitter = page.getByRole("separator", { name: "Resize Traffic records panel" });
  await splitter.focus();
  await splitter.press("ArrowRight");
  await expect(splitter).toHaveAttribute("aria-valuenow", "496");

  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(page.getByRole("region", { name: "Traffic record details" })).toBeVisible();

  await page.getByRole("button", { name: "Select" }).click();
  await page.getByRole("button", { name: "Select POST relay.example.test/v1/responses" }).click();
  await page.getByRole("button", { name: "Delete selected" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: "Delete selected" })).toBeFocused();
});
