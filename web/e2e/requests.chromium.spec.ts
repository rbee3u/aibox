import { expect, test } from "@playwright/test";
import { mockRequests } from "./requests.fixture";

for (const hasTouch of [false, true]) {
  test.describe(`Pretty body controls with ${hasTouch ? "coarse" : "fine"} pointers`, () => {
    test.use({ hasTouch });

    test("have consistent hit areas and visible interaction feedback", async ({ page }) => {
      await mockRequests(page);
      await page.goto("./");
      await page
        .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
        .click();

      let controlSize: number | undefined;
      for (const [tab, name] of [
        ["Request", "Collapse JSON root"],
        ["Request", "Copy object value"],
        ["Response", "Copy SSE Event data"],
      ]) {
        await page.getByRole("tab", { name: tab, exact: true }).click();
        const control = page.getByRole("button", { name, exact: true });
        await expect(control).toBeVisible();
        const bounds = await control.boundingBox();
        expect(bounds, name).not.toBeNull();
        // Minimum pointer targets are a usability boundary, not a CSS token snapshot.
        expect(bounds!.width, name).toBeGreaterThanOrEqual(hasTouch ? 44 : 24);
        expect(bounds!.height, name).toBe(bounds!.width);
        controlSize ??= bounds!.width;
        expect(bounds!.width, name).toBe(controlSize);

        await page.mouse.move(0, 0);
        const background = () =>
          control.evaluate((element) => getComputedStyle(element).backgroundColor);
        const resting = await background();
        await control.hover();
        await expect.poll(background, { message: `${name}: hover feedback` }).not.toBe(resting);
        const hovered = await background();
        await page.mouse.down();
        try {
          await expect.poll(background, { message: `${name}: press feedback` }).not.toBe(hovered);
        } finally {
          // Release outside the control so this geometry check does not toggle or copy.
          await page.mouse.move(0, 0);
          await page.mouse.up();
        }
        await page.keyboard.press("Tab");
        await control.focus();
        await expect(control).toBeFocused();
        await expect
          .poll(
            () =>
              control.evaluate((element) => {
                const style = getComputedStyle(element);
                return style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0;
              }),
            { message: `${name}: keyboard focus ring` },
          )
          .toBe(true);
      }
    });
  });
}

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
