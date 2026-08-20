import { expect, test } from "@playwright/test";
import { mockRequests } from "./requests.fixture";

test("sidebar utilities adapt across desktop and mobile layouts", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockRequests(page);
  await page.goto("./");

  const sidebar = page.getByRole("complementary", { name: "Console navigation" });
  await expect(sidebar).toBeVisible();
  await expect(page.getByRole("link", { name: "GitHub repo" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Codex docs" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Claude docs" })).toBeVisible();
  await expect(page.getByText("vtest")).toBeVisible();

  await page.getByRole("button", { name: "Color theme: System" }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  await expect(page.getByRole("link", { name: "GitHub repo" })).toHaveAttribute(
    "title",
    "GitHub repo",
  );
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("aibox-console-sidebar-collapsed")))
    .toBe("true");
  await expect.poll(async () => (await sidebar.boundingBox())?.width).toBeLessThan(65);

  await page.reload();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Color theme: Dark" })).toBeVisible();

  await page.setViewportSize({ width: 820, height: 720 });
  await page.getByRole("button", { name: "Open navigation" }).click();
  await expect(page.getByRole("link", { name: "Claude docs" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Color theme: Dark" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeHidden();
  await expect.poll(async () => (await sidebar.boundingBox())?.width).toBeGreaterThan(270);
});
