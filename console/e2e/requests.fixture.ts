import type { Page, Route } from "@playwright/test";
import type {
  AssessmentPrimary,
  ProtocolSummary,
  RequestAssessment,
  RequestDetail,
  RequestList,
  RequestSummary,
} from "../src/types";

const requestBody = '{"model":"gpt-5.6-sol"}';
const responseBody = 'data: {"type":"response.completed"}\n\n';

const protocol = {
  family: "openai_responses",
  response_terminal: true,
  model: { requested: "gpt-5.6-sol", effective: "gpt-5.6-sol" },
  reasoning_effort: { requested: "high", effective: "high" },
  response_mode: { requested: "stream", observed: "stream" },
  first_token_at_ns: "200000000",
  token_usage: null,
  errors: [],
  warnings: [],
} satisfies ProtocolSummary;

export const providerError = {
  source: "provider",
  kind: "server_error",
  message: "Our servers are currently overloaded. Please try again later.",
} satisfies AssessmentPrimary;

const assessment = {
  level: "error",
  primary: providerError,
  issue_count: 1,
} satisfies RequestAssessment;

const primaryRequest = {
  id: "019fe51f-82b7-7701-bfb0-231441977e27",
  started_at: "2026-08-09T06:04:45Z",
  ended_at: "2026-08-09T06:04:45.500Z",
  method: "POST",
  incoming_uri: "/https://relay.example.test/v1/responses",
  upstream_url: "https://relay.example.test/v1/responses",
  status: 200,
  http_version: "HTTP/2",
  outcome: "completed",
  state: "completed",
  total_ms: 500,
  protocol,
  assessment,
} satisfies RequestSummary;

const detail = {
  request: {
    id: primaryRequest.id,
    started_at: primaryRequest.started_at,
    method: primaryRequest.method,
    incoming_uri: primaryRequest.incoming_uri,
    upstream_url: primaryRequest.upstream_url,
    http_version: "HTTP/2.0",
    headers: [
      header("content-type", "application/json"),
      header("authorization", "Bearer test-token-not-a-secret"),
    ],
  },
  response: {
    status: 200,
    source: "upstream",
    headers_at: "2026-08-09T06:04:45.100Z",
    http_version: "HTTP/2",
    reason_phrase: "OK",
    headers: [header("content-type", "text/event-stream")],
  },
  result: {
    ended_at: primaryRequest.ended_at,
    outcome: "completed",
    total_ms: primaryRequest.total_ms,
    error: null,
  },
  summary: {
    schema_version: 1,
    request_id: primaryRequest.id,
    kind: "summary",
    observed_at: primaryRequest.started_at,
    request: {
      method: primaryRequest.method,
      incoming_uri: primaryRequest.incoming_uri,
      upstream_url: primaryRequest.upstream_url,
      http_version: "HTTP/2.0",
    },
    response: { status: 200, http_version: "HTTP/2" },
    terminal: true,
    timing: {
      upstream_request_started_at_ns: "10000000",
      upstream_request_body_first_byte_at_ns: "20000000",
      upstream_request_body_completed_at_ns: "30000000",
      upstream_response_headers_at_ns: "100000000",
      upstream_response_body_first_byte_at_ns: "200000000",
      upstream_response_body_completed_at_ns: "450000000",
      finished_at_ns: "500000000",
    },
    coding_agent_session_id: null,
    protocol,
    outcome: "completed",
    errors: [],
    warnings: [],
    assessment,
  },
  assessment,
  diagnostics: {
    request: [],
    http: [],
    provider: [{ ...providerError, level: "error", phase: "response", at_ns: "450000000" }],
    warnings: [],
  },
  state: "completed",
  request_body_bytes: byteLength(requestBody),
  response_body_bytes: byteLength(responseBody),
  live_total_ms: null,
  timeline_end_at_ns: "500000000",
} satisfies RequestDetail;

export async function mockRequests(
  page: Page,
  {
    total = 1,
    hasNext = false,
    sessionId = null,
  }: { total?: number; hasNext?: boolean; sessionId?: string | null } = {},
) {
  const requestList = {
    requests: [primaryRequest],
    total,
    deletable_count: 1,
    has_next: hasNext,
  } satisfies RequestList;
  await page.route("**/_aibox/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/_aibox/api/bootstrap") {
      return route.fulfill({ json: { version: "test", csrf_token: "test-token" } });
    }
    if (path === "/_aibox/api/operations/current") {
      return route.fulfill({ json: { operation: null, gap: false } });
    }
    if (path === "/_aibox/api/operations/events") {
      return route.fulfill({
        contentType: "text/event-stream",
        body: 'event: operation\ndata: {"operation":null}\n\n',
      });
    }
    if (path === "/_aibox/api/requests") return route.fulfill({ json: requestList });
    if (path === `/_aibox/api/requests/${primaryRequest.id}`) {
      return route.fulfill({
        json: {
          ...detail,
          summary: { ...detail.summary, coding_agent_session_id: sessionId },
        },
      });
    }
    if (path.endsWith("/request-body") || path.endsWith("/request-body-decoded")) {
      return fulfillBody(route, requestBody);
    }
    if (path.endsWith("/response-body") || path.endsWith("/response-body-decoded")) {
      return fulfillBody(route, responseBody);
    }
    if (path.endsWith("/response-event-timings")) {
      return route.fulfill({
        json: {
          state: "available",
          events: [{ sequence: 0, completed_at_ns: protocol.first_token_at_ns }],
          next_sequence: 1,
          warning: null,
        },
      });
    }
    throw new Error(`Unexpected Control API request: ${path}`);
  });
}

function fulfillBody(route: Route, body: string) {
  return route.fulfill({
    contentType: "application/octet-stream",
    headers: { "X-Aibox-Request-Next-Offset": String(byteLength(body)) },
    body,
  });
}

function byteLength(value: string) {
  return new TextEncoder().encode(value).length;
}

function header(name: string, value: string) {
  return { name, value_base64: btoa(value) };
}
