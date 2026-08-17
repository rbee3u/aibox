import { expect, test, type Page } from "@playwright/test";
import { mockRequests, providerError } from "./requests.fixture";

test("light desktop inspector", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await setTheme(page, "light");
  await mockRequests(page);
  await page.goto("./");
  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(page.getByText("gpt-5.6-sol").first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(page.getByRole("separator")).toHaveCount(0);

  const issueMarker = page.getByRole("img", { name: /Record error: Server error/ });
  await issueMarker.hover();
  const tooltip = page.getByRole("tooltip");
  await expect(tooltip).toContainText("Error · Server error");
  await expect(tooltip).toContainText(providerError.message);
  await page.mouse.move(900, 40);
  await expect(tooltip).toBeHidden();

  await issueMarker.hover();
  await expect(tooltip).toBeVisible();
  await page
    .getByRole("complementary", { name: "Request Record list" })
    .locator("[aria-busy]")
    .dispatchEvent("scroll");
  await expect(tooltip).toBeHidden();

  await page.getByText("Server error", { exact: true }).hover();
  await expect(tooltip).toContainText("Error · Server error");
  await expect(tooltip).toContainText(providerError.message);
  await page.mouse.move(900, 40);
  await expect(tooltip).toBeHidden();

  await page.getByRole("tab", { name: "Request" }).click();
  await expect(page.getByText("Bearer test-token-not-a-secret")).toBeVisible();
  await page.getByRole("tab", { name: "Response" }).click();
  await expect(page.getByLabel("SSE Events")).toBeVisible();
});

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.addInitScript((value) => localStorage.setItem("aibox-console-theme", value), theme);
}
