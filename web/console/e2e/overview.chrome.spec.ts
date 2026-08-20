import { expect, test } from "@playwright/test";
import { mockOverview } from "./overview.fixture";

test("Overview exposes the complete resource map before Session discovery", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const { sessionRequests } = await mockOverview(page);
  await page.goto("../overview");

  const status = page.getByRole("region", { name: "Service status" });
  await expect(status.getByText("Managed Tenants")).toBeVisible();
  await expect(status.getByText("Console-only view of the Host Home")).toBeVisible();
  await expectTypography(page.getByRole("heading", { name: "Key facts" }), {
    fontSize: "16px",
    fontWeight: "600",
    lineHeight: "22px",
  });
  await expectTypography(page.getByRole("banner").locator("small"), {
    fontSize: "12px",
    fontWeight: "400",
    lineHeight: "18px",
  });

  const tree = page.getByRole("tree", { name: "Tenant resource topology" });
  await expect(tree).toBeVisible();
  await expect(tree.getByText("Host Tenant")).toBeVisible();
  await expect(tree.getByRole("link", { name: /^default / })).toBeVisible();
  await expect(tree.getByText("Current Config").first()).toBeVisible();
  await expect(tree.getByText("daily").first()).toBeVisible();
  await expect(tree.getByText("Rust")).toBeVisible();
  expect(sessionRequests).toHaveLength(0);

  await page.getByRole("button", { name: "Expand Sessions" }).first().click();
  await expect(tree.getByText("3 Sessions").first()).toBeVisible();
  expect(sessionRequests).toHaveLength(1);
});

test("390px Overview keeps page overflow contained inside the topology viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockOverview(page);
  await page.goto("../overview");

  await expect(page.getByRole("heading", { name: "Resource topology" })).toBeVisible();
  await expect(page.getByRole("tree", { name: "Tenant resource topology" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Fit topology to width" })).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);

  const topologyViewport = page.locator("[data-topology-viewport]");
  await expect
    .poll(() => topologyViewport.evaluate((element) => element.scrollWidth > element.clientWidth))
    .toBe(true);
});

async function expectTypography(
  locator: import("@playwright/test").Locator,
  expected: { fontSize: string; fontWeight: string; lineHeight: string },
) {
  expect(
    await locator.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        fontSize: style.fontSize,
        fontWeight: style.fontWeight,
        lineHeight: style.lineHeight,
      };
    }),
  ).toMatchObject(expected);
}
