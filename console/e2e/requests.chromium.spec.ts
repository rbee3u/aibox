import { expect, test } from "@playwright/test";
import { mockRequests } from "./requests.fixture";

test("Request inspection preserves responsive and keyboard workflows", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockRequests(page);
  await page.goto("./");

  await page.getByRole("button", { name: "Color theme: System" }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  const requestList = page.getByRole("complementary", { name: "Request list" });
  const request = page.getByRole("button", {
    name: "POST relay.example.test/v1/responses",
    exact: true,
  });
  await request.click();
  await expect(page.getByRole("region", { name: "Request details" })).toBeVisible();

  await page.setViewportSize({ width: 760, height: 720 });
  await expect(requestList).toBeHidden();
  await page.getByRole("button", { name: "Back to Request list" }).click();
  await expect(requestList).toBeVisible();
  await expect(request).toBeFocused();

  await page.setViewportSize({ width: 761, height: 720 });
  await request.click();
  await expect(requestList).toBeVisible();
  await expect(page.getByRole("button", { name: "Back to Request list" })).toBeHidden();

  await page.getByRole("button", { name: "Select Requests" }).click();
  await page.getByRole("button", { name: "Select POST relay.example.test/v1/responses" }).click();
  await page.getByRole("button", { name: "Delete selected" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: "Delete selected" })).toBeFocused();
});
