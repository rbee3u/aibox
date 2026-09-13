import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

const startedAt = "2026-08-19T01:00:00Z";

function operation(over: Record<string, unknown> = {}) {
  return {
    id: "op-1",
    kind: "Install Rust toolchain",
    state: "running",
    started_at: startedAt,
    ended_at: null,
    result: null,
    first_sequence: 0,
    next_sequence: 0,
    logs: [] as Array<{ sequence: number; message: string }>,
    ...over,
  };
}

function logsUpTo(count: number) {
  return Array.from({ length: count }, (_, index) => ({
    sequence: index,
    message: `[${String(index).padStart(3, "0")}] compiling crate number ${index}`,
  }));
}

async function mount(
  page: Page,
  first: ReturnType<typeof operation>,
  growing: string[] = [],
  onRefresh = first,
) {
  let refreshed = false;
  await page.route("**/_aibox/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({
        json: { version: "test", csrf_token: "test-token", listen: "127.0.0.1:3000" },
      });
    }
    if (path === "/_aibox/api/operations/current") {
      const body = refreshed ? onRefresh : first;
      refreshed = true;
      return route.fulfill({ json: { operation: body, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({ contentType: "text/event-stream", body: growing.join("") });
    }
    if (path === "/_aibox/api/tenants") {
      return route.fulfill({
        json: Array.from({ length: 14 }, (_, index) => ({
          kind: "managed",
          name: `tenant-${index}`,
          display_name: `tenant-${index}`,
          home: `/tenants/tenant-${index}`,
          exists: true,
        })),
      });
    }
    if (path === "/_aibox/api/components") return route.fulfill({ json: [] });
    if (path === "/_aibox/api/components/latest") return route.fulfill({ json: null });
    return route.fulfill({ json: {} });
  });
}

const panelSelector = 'section[aria-label="Management Operation"]';

test("the collapsed bar reserves its own space instead of covering the catalog", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  // A succeeded Operation starts collapsed, so this is the ordinary state after
  // any successful install rather than an unusual one.
  await mount(
    page,
    operation({
      state: "succeeded",
      ended_at: "2026-08-19T01:04:12Z",
      result: "Installed rust 1.83.0",
    }),
  );
  await page.goto("../tenants");
  await page.waitForSelector(panelSelector);
  await expect(page.getByRole("button", { name: "Expand operation" })).toBeVisible();

  // The catalog scrolls, so the test is whether its end can be brought into
  // view above the panel, not whether it already sits there.
  const reach = await page.evaluate((selector) => {
    const panel = document.querySelector(selector) as HTMLElement;
    const rows = [...document.querySelectorAll("button")].filter((button) =>
      /^tenant-\d+/.test(button.innerText.trim()),
    );
    const last = rows[rows.length - 1] as HTMLElement;
    const scroller = last.closest('[class*="list"]') as HTMLElement;
    scroller.scrollTop = scroller.scrollHeight;
    const box = last.getBoundingClientRect();
    const atCentre = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return {
      lastRowBottom: box.bottom,
      panelTop: panel.getBoundingClientRect().top,
      lastRowIsOnTop: last === atCentre || last.contains(atCentre),
    };
  }, panelSelector);

  expect(reach.lastRowBottom).toBeLessThanOrEqual(reach.panelTop);
  expect(reach.lastRowIsOnTop).toBe(true);
});

test("the log follows a growing stream until the reader scrolls away", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const frames = [20, 50, 90].map(
    (count) =>
      `event: operation\ndata: ${JSON.stringify({
        operation: operation({ logs: logsUpTo(count), next_sequence: count }),
        gap: false,
      })}\n\n`,
  );
  await mount(
    page,
    operation({ logs: logsUpTo(20), next_sequence: 20 }),
    frames,
    operation({ logs: logsUpTo(130), next_sequence: 130 }),
  );
  await page.goto("../tenants");
  await page.waitForSelector(`${panelSelector} pre`);
  await expect(page.locator(`${panelSelector} pre`)).toContainText("crate number 89");

  const followed = await page.evaluate((selector) => {
    const log = document.querySelector(`${selector} pre`) as HTMLElement;
    return {
      atBottom: log.scrollTop + log.clientHeight >= log.scrollHeight - 8,
      scrollable: log.scrollHeight > log.clientHeight,
    };
  }, panelSelector);
  expect(followed.scrollable).toBe(true);
  expect(followed.atBottom).toBe(true);

  // Reading history is deliberate, so a frame that grows the log must not yank
  // the view back down.
  await page.evaluate((selector) => {
    (document.querySelector(`${selector} pre`) as HTMLElement).scrollTop = 0;
  }, panelSelector);
  await page.getByRole("button", { name: "Refresh operation" }).click();
  await expect(page.locator(`${panelSelector} pre`)).toContainText("crate number 129");
  const held = await page.evaluate(
    (selector) => (document.querySelector(`${selector} pre`) as HTMLElement).scrollTop,
    panelSelector,
  );
  expect(held).toBe(0);
});
