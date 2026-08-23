import { expect, test } from "@playwright/test";
import { mockCodexVisual, mockConfigs, mockConfigWorkflows } from "./configs.fixture";

test("Codex Visual Config uses compact rows and closed enum controls", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => localStorage.setItem("aibox-console-theme", "light"));
  await mockCodexVisual(page);
  await page.goto("../configs?tenant=managed%3Adefault&agent=codex&config=team&file=config.toml");

  const approval = page.getByRole("combobox", { name: "Approval policy value" });
  await expect(approval).toBeVisible();
  await expect(approval.locator("option")).toHaveText(["untrusted", "on-request", "never"]);
  await expect(page.getByText("approval_policy", { exact: true })).toHaveCount(0);

  const label = page.getByText("Approval policy", { exact: true });
  const labelBox = await label.boundingBox();
  const controlBox = await approval.boundingBox();
  expect(labelBox).not.toBeNull();
  expect(controlBox).not.toBeNull();
  expect(controlBox!.x).toBeGreaterThan(labelBox!.x + labelBox!.width);
  expect(
    Math.abs(controlBox!.y + controlBox!.height / 2 - (labelBox!.y + labelBox!.height / 2)),
  ).toBeLessThan(8);

  const executionGroup = page
    .getByRole("heading", { name: "Execution & permissions" })
    .locator("../..");
  const groupBox = await executionGroup.boundingBox();
  expect(groupBox).not.toBeNull();
  expect(groupBox!.height).toBeLessThan(180);

  await page.getByRole("button", { name: "Help for Approval policy" }).focus();
  await expect(page.getByRole("tooltip")).toContainText(
    "Controls when Codex pauses before executing commands.",
  );
});

test("Raw Config editor aligns line numbers and highlights TOML", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => localStorage.setItem("aibox-console-theme", "light"));
  await mockConfigs(page);
  await page.goto("../configs?tenant=managed%3Adefault&agent=codex&file=config.toml");

  await expect
    .poll(() =>
      page.evaluate(() => {
        const root = document.querySelector<HTMLElement>("#root");
        const app = document.querySelector<HTMLElement>("[data-aibox-shell]");
        if (!root || !app) return false;
        return (
          Math.abs(root.getBoundingClientRect().height - window.innerHeight) < 1 &&
          Math.abs(app.getBoundingClientRect().height - window.innerHeight) < 1
        );
      }),
    )
    .toBe(true);

  const editor = page.locator(".cm-editor");
  await expect(editor).toBeVisible();
  await expect(page.locator(".cm-scroller")).toHaveCSS("display", "flex");

  const firstLine = page.locator(".cm-line").first();
  const firstNumber = page
    .locator(".cm-lineNumbers .cm-gutterElement")
    .filter({ hasText: /^1$/ })
    .first();
  await expect(firstLine).toBeVisible();
  await expect(firstNumber).toBeVisible();
  await expect
    .poll(async () => {
      const lineTop = await textTop(firstLine);
      const numberTop = await textTop(firstNumber);
      return Math.abs(lineTop - numberTop);
    })
    .toBeLessThan(1);

  await expect(page.locator(".cm-config-key").first()).toHaveCSS("color", "rgb(40, 94, 168)");
  await expect(page.locator(".cm-config-string").first()).toHaveCSS("color", "rgb(57, 117, 45)");
  await expect(page.locator(".cm-config-number").first()).toHaveCSS("color", "rgb(156, 77, 17)");
  await expect(page.locator(".cm-config-boolean").first()).toHaveCSS("color", "rgb(121, 80, 167)");
});

test("390px Config editing keeps Named and Current workflows explicit", async ({ page }) => {
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
  const baseLabelBox = await page.getByText("Anthropic base URL", { exact: true }).boundingBox();
  const baseInputBox = await page
    .getByRole("textbox", { name: "Anthropic base URL" })
    .boundingBox();
  expect(baseLabelBox).not.toBeNull();
  expect(baseInputBox).not.toBeNull();
  expect(baseInputBox!.y).toBeGreaterThan(baseLabelBox!.y);
  await page.getByRole("button", { name: "Raw" }).click();
  await expect(page.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".cm-editor")).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);

  await page.getByRole("button", { name: "Back to Configs" }).click();
  const team = page.getByRole("button", { name: "team", exact: true });
  await expect(team).toBeVisible();
  await expect(team).toBeFocused();

  await page.goto("../configs?tenant=managed%3Adefault&agent=claude&current=1&file=settings.json");
  await expect(page.getByRole("button", { name: "Raw" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Visual" })).toHaveCount(0);
  await expect(page.getByLabel("Config editing context")).toContainText("Current Config");
});

async function textTop(locator: import("@playwright/test").Locator) {
  return locator.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    return range.getBoundingClientRect().top;
  });
}
