import { expect, test, type Locator, type Page } from "@playwright/test";
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

  const issueMarker = page.getByRole("img", { name: /Request error: Server error/ });
  await issueMarker.hover();
  const tooltip = page.getByRole("tooltip");
  await expect(tooltip).toContainText("Error · Server error");
  await expect(tooltip).toContainText(providerError.message);
  await page.mouse.move(900, 40);
  await expect(tooltip).toBeHidden();

  await issueMarker.hover();
  await expect(tooltip).toBeVisible();
  await page
    .getByRole("complementary", { name: "Request list" })
    .locator(":scope > [aria-busy]")
    .dispatchEvent("scroll");
  await expect(tooltip).toBeHidden();

  await page.getByRole("button", { name: "Error: Server error" }).hover();
  await expect(tooltip).toContainText("Error · Server error");
  await expect(tooltip).toContainText(providerError.message);
  await page.mouse.move(900, 40);
  await expect(tooltip).toBeHidden();

  await page.getByRole("tab", { name: "Request" }).click();
  await expect(page.getByText("Bearer test-token-not-a-secret")).toBeVisible();
  await page.getByRole("tab", { name: "Response" }).click();
  await expect(page.getByLabel("SSE Events")).toBeVisible();
});

test("390px Request inspection keeps the complete workflow in one panel", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockRequests(page, { total: 101, hasNext: true });
  await page.goto("./");

  await expect(page.getByText("Page 1 of 3")).toBeVisible();
  await expect(page.locator('[title="Ended 2026-08-09 14:04:45"]')).toBeVisible();

  const row = page.getByRole("button", {
    name: "POST relay.example.test/v1/responses",
    exact: true,
  });
  await row.click();
  await expect(page.getByRole("region", { name: "Request details" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Summary" })).toBeVisible();
  await page.getByRole("tab", { name: "Request" }).click();
  await expect(page.getByText("Bearer test-token-not-a-secret")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Response" })).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);

  await page.getByRole("button", { name: "Back to Request list" }).click();
  await expect(page.getByRole("complementary", { name: "Request list" })).toBeVisible();
  await expect(row).toBeFocused();
});

test("shared typography distinguishes catalog metadata from technical values", async ({ page }) => {
  const sessionId = "019fe51f-82b7-7701-bfb0-typography";
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockRequests(page, { sessionId });
  await mockSessionCatalog(page);
  await page.goto("./");

  const requestRow = page.getByRole("button", {
    name: "POST relay.example.test/v1/responses",
    exact: true,
  });
  const requestTarget = requestRow.locator("strong").first();
  const requestModel = requestRow.getByTitle("Model gpt-5.6-sol; Reasoning effort high");
  const requestTiming = requestRow.getByTitle("First token 200ms; Duration 500ms");
  const requestTime = requestRow.locator("time");
  const [requestTargetStyle, requestModelStyle, requestTimingStyle, requestTimeStyle] =
    await Promise.all(
      [requestTarget, requestModel, requestTiming, requestTime].map((locator) =>
        typography(locator),
      ),
    );

  expect(requestModelStyle).toMatchObject({
    fontSize: "12px",
    fontWeight: "400",
    lineHeight: "18px",
  });
  expect(requestModelStyle.fontFamily).toContain("ui-sans-serif");
  expect(requestTimingStyle.fontFamily).toContain("ui-monospace");
  expect(requestTimingStyle.fontVariantNumeric).toContain("tabular-nums");
  expect(requestTimeStyle).toMatchObject({
    fontFamily: requestTimingStyle.fontFamily,
    fontSize: "12px",
    fontWeight: "400",
    lineHeight: "18px",
  });

  await requestRow.click();
  const sessionValue = page.getByText(sessionId, { exact: true });
  await expect(sessionValue).toBeVisible();
  expect(await typography(sessionValue)).toMatchObject({
    fontFamily: requestTimingStyle.fontFamily,
    fontSize: "12px",
    fontWeight: "500",
  });

  await page.getByRole("button", { name: "Delete POST relay.example.test/v1/responses" }).click();
  const dialogHeading = page.getByRole("dialog").getByRole("heading");
  expect(await typography(dialogHeading)).toMatchObject({
    fontSize: "16px",
    fontWeight: "600",
    lineHeight: "22px",
  });
  await page.getByRole("button", { name: "Cancel" }).click();

  await page.goto("/_aibox/ui/sessions?tenant=managed%3Adefault&agent=codex");
  const sessionRow = page.getByRole("button", {
    name: "Typography prompt, Tenant default · Codex",
  });
  const sessionTitle = sessionRow.locator("strong");
  const sessionMetadata = sessionRow.locator("small > span");
  const sessionTime = sessionRow.locator("small > time");
  const [sessionTitleStyle, sessionMetadataStyle, sessionTimeStyle] = await Promise.all(
    [sessionTitle, sessionMetadata, sessionTime].map((locator) => typography(locator)),
  );

  expect(sessionMetadataStyle).toMatchObject({
    fontFamily: requestModelStyle.fontFamily,
    fontSize: requestModelStyle.fontSize,
    fontWeight: requestModelStyle.fontWeight,
    lineHeight: requestModelStyle.lineHeight,
  });
  expect(sessionTimeStyle).toMatchObject({
    fontFamily: requestTimingStyle.fontFamily,
    fontSize: requestTimingStyle.fontSize,
    fontWeight: requestTimingStyle.fontWeight,
    lineHeight: requestTimingStyle.lineHeight,
  });
  expect(sessionTimeStyle.fontVariantNumeric).toContain("tabular-nums");
  expect(sessionTitleStyle.fontFamily).toBe(requestTargetStyle.fontFamily);
});

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.addInitScript((value) => localStorage.setItem("aibox-console-theme", value), theme);
}

async function mockSessionCatalog(page: Page) {
  await page.route("**/_aibox/api/**", (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: [
          {
            kind: "managed",
            name: "default",
            display_name: "default",
            home: "/tenants/default/home",
            exists: true,
          },
        ],
      });
    }
    if (path === "/_aibox/api/sessions" && request.method() === "GET") {
      return route.fulfill({
        json: {
          sessions: [
            {
              id: "session-typography",
              display_id: "session-typography",
              start_ts: "2026-08-17T09:00:00Z",
              title: "Typography prompt",
              warnings: [],
            },
          ],
          warnings: [],
          partial: false,
        },
      });
    }
    return route.fallback();
  });
}

async function typography(locator: Locator) {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      fontFamily: style.fontFamily,
      fontSize: style.fontSize,
      fontWeight: style.fontWeight,
      fontVariantNumeric: style.fontVariantNumeric,
      lineHeight: style.lineHeight,
    };
  });
}
