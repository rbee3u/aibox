import { expect, test } from "@playwright/test";

const id = "019fe51f-82b7-7701-bfb0-231441977e27";
const record = {
  id,
  started_at: "2026-08-09T06:04:45Z",
  method: "GET",
  incoming_uri: "/https://example.test/api/hello",
  upstream_url: "https://example.test/api/hello",
  status: 200,
  http_version: "HTTP/2",
  outcome: "completed",
  state: "completed",
  total_ms: 420,
  protocol: null,
};
const detail = {
  request: {
    id,
    started_at: record.started_at,
    method: record.method,
    incoming_uri: record.incoming_uri,
    upstream_url: record.upstream_url,
    http_version: "HTTP/2.0",
    headers: [],
  },
  response: {
    status: 200,
    source: "upstream",
    headers_at: "2026-08-09T06:04:45.200Z",
    http_version: "HTTP/2",
    reason_phrase: "OK",
    headers: [],
  },
  result: {
    ended_at: "2026-08-09T06:04:45.420Z",
    outcome: "completed",
    total_ms: 420,
    error: null,
  },
  summary: {
    schema_version: 1,
    record_id: id,
    kind: "summary",
    observed_at: record.started_at,
    terminal: true,
    timing: {
      upstream_request_started_at_ns: "20000000",
      upstream_request_body_first_byte_at_ns: null,
      upstream_request_body_completed_at_ns: "30000000",
      upstream_response_headers_at_ns: "200000000",
      upstream_response_body_first_byte_at_ns: "220000000",
      upstream_response_body_completed_at_ns: "410000000",
      finished_at_ns: "420000000",
    },
    coding_agent_session_id: null,
    protocol: null,
    outcome: "completed",
    errors: [],
    warnings: [],
  },
  state: "completed",
  request_body_bytes: 0,
  response_body_bytes: 0,
  live_total_ms: null,
  timeline_end_at_ns: "420000000",
};

test("desktop layout and keyboard interactions", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.route("**/_aibox/traffic/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/_aibox/traffic/api/records") {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          records: [record],
          total: 1,
          deletable_count: 1,
          next_cursor: null,
        }),
      });
    }
    if (path === `/_aibox/traffic/api/records/${id}`) {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(detail),
      });
    }
    return route.fulfill({
      status: 200,
      contentType: "application/octet-stream",
      headers: { "X-Aibox-Traffic-Next-Offset": "0" },
      body: "",
    });
  });
  await page.goto("./");

  await page.getByRole("combobox", { name: "Color theme" }).selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  const splitter = page.getByRole("separator", { name: "Resize Traffic records panel" });
  await splitter.focus();
  await splitter.press("ArrowRight");
  await expect(splitter).toHaveAttribute("aria-valuenow", "496");

  await page.getByRole("button", { name: "GET example.test/api/hello", exact: true }).click();
  await expect(page.getByRole("region", { name: "Traffic record details" })).toBeVisible();
  await expect(page.getByText("Token usage is unavailable for this protocol.")).toBeVisible();

  await page.getByRole("button", { name: "Select" }).click();
  await page.getByRole("button", { name: "Select GET example.test/api/hello" }).click();
  await page.getByRole("button", { name: "Delete selected" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.press("Escape");
  await expect(dialog).toBeHidden();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.tagName,
      ),
    )
    .toBe("Delete selected");
});
