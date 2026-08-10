import { expect, test, type Page, type Route } from "@playwright/test";

const requestBody = JSON.stringify(
  {
    model: "gpt-5.6-sol",
    input: [{ role: "user", content: "Inspect this repository and explain the failing check." }],
    reasoning: { effort: "high" },
    stream: true,
  },
  null,
  2,
);

const responseBody = [
  'event: response.created\ndata: {"type":"response.created","model":"gpt-5.6-sol"}\n\n',
  'event: response.output_text.delta\ndata: {"type":"response.output_text.delta","delta":"The check fails because…"}\n\n',
  'event: response.completed\ndata: {"type":"response.completed","usage":{"input_tokens":9896,"output_tokens":1119}}\n\n',
].join("");

const protocol = {
  family: "openai_responses",
  response_terminal: true,
  model: { requested: "gpt-5.6-sol", effective: "gpt-5.6-sol" },
  reasoning_effort: { requested: "high", effective: "high" },
  response_mode: { requested: "stream", observed: "stream" },
  first_token_at_ns: "2110000000",
  token_usage: {
    total_input_tokens: 9896,
    base_input_tokens: 9896,
    cached_input_tokens: 0,
    cache_write_tokens: 0,
    cache_write_5m_tokens: null,
    cache_write_1h_tokens: null,
    output_tokens: 1119,
    reasoning_output_tokens: 1034,
  },
  errors: [],
  warnings: [],
};

const okAssessment = { level: "ok", primary: null, issue_count: 0 };
const activeAssessment = { level: "active", primary: null, issue_count: 0 };
const providerError = {
  source: "provider",
  kind: "server_error",
  message: "Our servers are currently overloaded. Please try again later.",
};
const providerErrorAssessment = { level: "error", primary: providerError, issue_count: 1 };

const primaryRecord = {
  id: "019fe51f-82b7-7701-bfb0-231441977e27",
  started_at: "2026-08-09T06:04:45Z",
  ended_at: "2026-08-09T06:05:12.830Z",
  method: "POST",
  incoming_uri: "/https://relay.example.test/v1/responses",
  upstream_url: "https://relay.example.test/v1/responses",
  status: 200,
  http_version: "HTTP/2",
  outcome: "completed",
  state: "completed",
  total_ms: 27830,
  protocol,
  assessment: providerErrorAssessment,
};

const records = [
  primaryRecord,
  record(
    "019fe51f-82b7-7702-bfb0-231441977e27",
    "POST",
    "relay.example.test",
    "/v1/messages",
    200,
    "claude-opus-5",
    40020,
  ),
  record(
    "019fe51f-82b7-7703-bfb0-231441977e27",
    "POST",
    "relay.example.test",
    "/v1/messages",
    200,
    "claude-opus-5",
    4320,
  ),
  record(
    "019fe51f-82b7-7704-bfb0-231441977e27",
    "HEAD",
    "relay.example.test",
    "/api/hello",
    404,
    null,
    913,
  ),
  record(
    "019fe51f-82b7-7705-bfb0-231441977e27",
    "POST",
    "assistant.example.test",
    "/backend-api/codex/responses",
    200,
    "gpt-5.6-sol",
    23100,
  ),
  record(
    "019fe51f-82b7-7706-bfb0-231441977e27",
    "GET",
    "assistant.example.test",
    "/backend-api/codex/models",
    200,
    null,
    980,
  ),
  record(
    "019fe51f-82b7-7707-bfb0-231441977e27",
    "POST",
    "models.example.test",
    "/v1/responses",
    null,
    "gpt-5.6-sol",
    4900,
    true,
  ),
];

const detail = {
  request: {
    id: primaryRecord.id,
    started_at: primaryRecord.started_at,
    method: primaryRecord.method,
    incoming_uri: primaryRecord.incoming_uri,
    upstream_url: primaryRecord.upstream_url,
    http_version: "HTTP/2.0",
    headers: [
      header("content-type", "application/json"),
      header("authorization", "Bearer test-token-not-a-secret"),
      header("session-id", primaryRecord.id),
      header("user-agent", "traffic-fixture/1.0"),
    ],
  },
  response: {
    status: 200,
    source: "upstream",
    headers_at: "2026-08-09T06:04:46Z",
    http_version: "HTTP/2",
    reason_phrase: "OK",
    headers: [header("content-type", "text/event-stream")],
  },
  result: {
    ended_at: primaryRecord.ended_at,
    outcome: "completed",
    total_ms: 27830,
    error: null,
  },
  summary: {
    schema_version: 1,
    record_id: primaryRecord.id,
    kind: "summary",
    observed_at: primaryRecord.started_at,
    request: {
      method: primaryRecord.method,
      incoming_uri: primaryRecord.incoming_uri,
      upstream_url: primaryRecord.upstream_url,
      http_version: "HTTP/2.0",
    },
    response: { status: 200, http_version: "HTTP/2" },
    terminal: true,
    timing: {
      upstream_request_started_at_ns: "119000000",
      upstream_request_body_first_byte_at_ns: "132000000",
      upstream_request_body_completed_at_ns: "784000000",
      upstream_response_headers_at_ns: "2081000000",
      upstream_response_body_first_byte_at_ns: "2110000000",
      upstream_response_body_completed_at_ns: "27813000000",
      finished_at_ns: "27830000000",
    },
    coding_agent_session_id: primaryRecord.id,
    protocol,
    outcome: "completed",
    errors: [],
    warnings: [],
    assessment: providerErrorAssessment,
  },
  assessment: providerErrorAssessment,
  diagnostics: {
    traffic: [],
    http: [],
    provider: [
      {
        ...providerError,
        level: "error",
        phase: "response",
        at_ns: "27813000000",
      },
    ],
    warnings: [],
  },
  state: "completed",
  request_body_bytes: new TextEncoder().encode(requestBody).length,
  response_body_bytes: new TextEncoder().encode(responseBody).length,
  live_total_ms: null,
  timeline_end_at_ns: "27830000000",
};

test("light desktop inspector", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await setTheme(page, "light");
  await mockTraffic(page);
  await page.goto("./");
  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(page.getByText("gpt-5.6-sol").first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(
    page.getByRole("separator", { name: "Resize Traffic records panel" }),
  ).toHaveAttribute("aria-valuenow", "480");
  await expectPaginationAtPanelBottom(page);

  const issueMarker = page.getByRole("img", { name: /Record error: Server error/ });
  const markerBox = await issueMarker.boundingBox();
  expect(markerBox).not.toBeNull();
  expect(markerBox!.width).toBeGreaterThanOrEqual(24);
  expect(markerBox!.height).toBeGreaterThanOrEqual(24);
  await issueMarker.hover();
  const tooltip = page.getByRole("tooltip");
  await expect(tooltip).toContainText("Error · Server error");
  await expect(tooltip).toContainText(providerError.message);
  const tooltipBox = await tooltip.boundingBox();
  expect(tooltipBox).not.toBeNull();
  expect(tooltipBox!.x).toBeGreaterThanOrEqual(8);
  expect(tooltipBox!.y).toBeGreaterThanOrEqual(8);
  expect(tooltipBox!.x + tooltipBox!.width).toBeLessThanOrEqual(1432);
  expect(tooltipBox!.y + tooltipBox!.height).toBeLessThanOrEqual(892);
  expect(tooltipBox!.x + tooltipBox!.width).toBeLessThan(markerBox!.x);
  await page.mouse.move(900, 40);
  await expect(tooltip).toBeHidden();

  await issueMarker.hover();
  await expect(tooltip).toBeVisible();
  await page
    .getByRole("complementary", { name: "Traffic records" })
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

test("dark desktop loading, selection, and dialog", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await setTheme(page, "dark");
  await mockTraffic(page, 700);
  await page.goto("./");
  await page
    .getByRole("button", { name: "POST relay.example.test/v1/responses", exact: true })
    .click();
  await expect(page.getByText("Loading record…")).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.getByText("gpt-5.6-sol").first()).toBeVisible();

  await page.getByRole("button", { name: "Select" }).click();
  await page.getByRole("button", { name: "Select POST relay.example.test/v1/responses" }).click();
  await page.getByRole("button", { name: "Delete selected" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("empty and error states remain scoped", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await setTheme(page, "light");
  await mockEmpty(page);
  await page.goto("./");
  await expect(page.getByText("No traffic recorded yet.")).toBeVisible();
  await expectPaginationAtPanelBottom(page);
  await expectEmptyStateCentered(page);

  await page.unrouteAll({ behavior: "wait" });
  await mockListError(page);
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("cannot scan Traffic Records");
  await expectPaginationAtPanelBottom(page);
  await expect(page.getByRole("heading", { name: "Select a request" })).toBeVisible();
});

async function mockTraffic(page: Page, detailDelay = 0) {
  await page.route("**/_aibox/traffic/api/**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (path === "/_aibox/traffic/api/records") {
      return json(route, { records, total: records.length, deletable_count: 6, has_next: false });
    }
    if (path === `/_aibox/traffic/api/records/${primaryRecord.id}`) {
      if (detailDelay) await new Promise((resolve) => setTimeout(resolve, detailDelay));
      return json(route, detail);
    }
    if (path.endsWith("/request-body") || path.endsWith("/request-body-decoded")) {
      return body(route, requestBody);
    }
    if (path.endsWith("/response-body") || path.endsWith("/response-body-decoded")) {
      return body(route, responseBody);
    }
    if (path.endsWith("/response-event-timings")) {
      return json(route, {
        state: "available",
        events: [
          { sequence: 0, completed_at_ns: "2110000000" },
          { sequence: 1, completed_at_ns: "5800000000" },
          { sequence: 2, completed_at_ns: "27813000000" },
        ],
        next_sequence: 3,
        warning: null,
      });
    }
    return json(route, { deleted: 1 });
  });
}

async function mockEmpty(page: Page) {
  await page.route("**/_aibox/traffic/api/records", (route) =>
    json(route, { records: [], total: 0, deletable_count: 0, has_next: false }),
  );
}

async function mockListError(page: Page) {
  await page.route("**/_aibox/traffic/api/records", (route) =>
    route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "cannot scan Traffic Records" }),
    }),
  );
}

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.addInitScript((value) => localStorage.setItem("aibox-traffic-theme", value), theme);
}

function record(
  id: string,
  method: string,
  host: string,
  path: string,
  status: number | null,
  model: string | null,
  totalMs: number,
  active = false,
) {
  return {
    id,
    started_at: "2026-08-09T06:01:26Z",
    ended_at: active ? null : "2026-08-09T06:02:26Z",
    method,
    incoming_uri: `/https://${host}${path}`,
    upstream_url: `https://${host}${path}`,
    status,
    http_version: status === null ? null : "HTTP/2",
    outcome: active ? "active" : "completed",
    state: active ? "active" : "completed",
    total_ms: totalMs,
    protocol: model
      ? {
          ...protocol,
          response_terminal: !active,
          model: { requested: model, effective: active ? null : model },
          first_token_at_ns: active ? null : "2300000000",
          token_usage: active ? null : protocol.token_usage,
        }
      : null,
    assessment: active ? activeAssessment : okAssessment,
  };
}

async function expectPaginationAtPanelBottom(page: Page) {
  const panel = page.getByRole("complementary", { name: "Traffic records" });
  const pagination = page.getByRole("navigation", { name: "Record pages" });
  await expect
    .poll(async () => {
      const [columnBox, paginationBox] = await Promise.all([
        panel.locator("..").boundingBox(),
        pagination.boundingBox(),
      ]);
      if (!columnBox || !paginationBox) return Number.POSITIVE_INFINITY;
      return Math.abs(columnBox.y + columnBox.height - (paginationBox.y + paginationBox.height));
    })
    .toBeLessThanOrEqual(1);
}

async function expectEmptyStateCentered(page: Page) {
  const empty = page.getByText("No traffic recorded yet.").locator("..");
  const records = page
    .getByRole("complementary", { name: "Traffic records" })
    .locator("[aria-busy]");
  const [emptyBox, recordsBox] = await Promise.all([empty.boundingBox(), records.boundingBox()]);
  expect(emptyBox).not.toBeNull();
  expect(recordsBox).not.toBeNull();
  expect(
    Math.abs(emptyBox!.y + emptyBox!.height / 2 - (recordsBox!.y + recordsBox!.height / 2)),
  ).toBeLessThanOrEqual(1);
}

function header(name: string, value: string) {
  return { name, value_base64: btoa(value) };
}

function json(route: Route, value: unknown) {
  return route.fulfill({
    status: 200,
    contentType: "application/json",
    body: JSON.stringify(value),
  });
}

function body(route: Route, value: string) {
  return route.fulfill({
    status: 200,
    contentType: "application/octet-stream",
    headers: { "X-Aibox-Traffic-Next-Offset": String(new TextEncoder().encode(value).length) },
    body: value,
  });
}
