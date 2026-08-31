import { expect, test } from "@playwright/test";
import { mockConfigWorkflows } from "./configs.fixture";

test("Config editing keeps Visual, Raw, Named, and Current modes explicit", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockConfigWorkflows(page);
  await page.goto(
    "../configs?tenant=managed%3Adefault&agent=claude&config=team&file=settings.json",
  );

  await expect(page.getByRole("button", { name: "Visual" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.getByLabel("Config editing context")).toContainText("team");
  await expect(page.getByRole("textbox", { name: "Anthropic base URL" })).toBeVisible();

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
