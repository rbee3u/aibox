import { vi } from "vitest";
import type {
  RecordAssessment,
  ProtocolSummary,
  RecordDetail,
  RecordList,
  RecordSummary,
  TrafficApi,
} from "../types";

export interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
}

export function deferred<T>(): Deferred<T> {
  let resolve!: Deferred<T>["resolve"];
  let reject!: Deferred<T>["reject"];
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

export const okAssessment = {
  level: "ok",
  primary: null,
  issue_count: 0,
} satisfies RecordAssessment;

export const activeAssessment = {
  level: "active",
  primary: null,
  issue_count: 0,
} satisfies RecordAssessment;

export const completedProtocol = {
  family: "openai_responses",
  response_terminal: true,
  model: { requested: "gpt-5.6-sol", effective: "gpt-5.6-sol" },
  reasoning_effort: { requested: "high", effective: "high" },
  response_mode: { requested: "stream", observed: "stream" },
  first_token_at_ns: "900000000",
  token_usage: {
    total_input_tokens: 12000,
    base_input_tokens: 2000,
    cached_input_tokens: 10000,
    cache_write_tokens: null,
    cache_write_5m_tokens: null,
    cache_write_1h_tokens: null,
    output_tokens: 320,
    reasoning_output_tokens: 64,
  },
  errors: [],
  warnings: [],
} satisfies ProtocolSummary;

export const completedChatProtocol = {
  ...completedProtocol,
  family: "openai_chat_completions",
  model: { requested: "gpt-chat", effective: "gpt-chat-2026-08-01" },
  reasoning_effort: { requested: "medium", effective: null },
  token_usage: {
    total_input_tokens: 150,
    base_input_tokens: 100,
    cached_input_tokens: 40,
    cache_write_tokens: 10,
    cache_write_5m_tokens: null,
    cache_write_1h_tokens: null,
    output_tokens: 20,
    reasoning_output_tokens: 5,
  },
} satisfies ProtocolSummary;

export const activeProtocol = {
  ...completedProtocol,
  response_terminal: false,
  model: { requested: "gpt-5.6-sol", effective: null },
  reasoning_effort: { requested: "high", effective: null },
  first_token_at_ns: null,
  token_usage: null,
} satisfies ProtocolSummary;

export const completedSummary = {
  id: "0198-demo-completed",
  started_at: "2026-08-06T04:00:00Z",
  ended_at: "2026-08-06T04:00:01.250Z",
  method: "POST",
  incoming_uri: "/https://api.example.test/v1/responses?stream=true",
  upstream_url: "https://api.example.test/v1/responses?stream=true",
  status: 200,
  http_version: "HTTP/2",
  outcome: "completed",
  state: "completed",
  total_ms: 1250,
  protocol: completedProtocol,
  assessment: okAssessment,
} satisfies RecordSummary;

export const activeSummary = {
  id: "0198-demo-active",
  started_at: "2026-08-06T04:01:00Z",
  ended_at: null,
  method: "GET",
  incoming_uri: "/https://stream.example.test/events",
  upstream_url: "https://stream.example.test/events",
  status: null,
  http_version: null,
  outcome: "active",
  state: "active",
  total_ms: 500,
  protocol: activeProtocol,
  assessment: activeAssessment,
} satisfies RecordSummary;

export function completedSummaryFor(id: string, host: string): RecordSummary {
  const upstreamUrl = `https://${host}/v1/responses`;
  return {
    ...completedSummary,
    id,
    incoming_uri: `/${upstreamUrl}`,
    upstream_url: upstreamUrl,
  };
}

export function recordListFor(
  records: RecordSummary[],
  overrides: Partial<Omit<RecordList, "records">> = {},
): RecordList {
  return {
    records,
    total: records.length,
    deletable_count: records.filter((record) => record.state !== "active").length,
    has_next: false,
    ...overrides,
  };
}

export const recordList = recordListFor([activeSummary, completedSummary]);

export const activeRecordList = recordListFor([activeSummary]);

export const completedDetail = {
  request: {
    id: completedSummary.id,
    started_at: completedSummary.started_at,
    method: completedSummary.method,
    incoming_uri: completedSummary.incoming_uri,
    upstream_url: completedSummary.upstream_url,
    http_version: "HTTP/2.0",
    headers: [{ name: "content-type", value_base64: btoa("application/json") }],
  },
  response: {
    status: 200,
    source: "upstream",
    headers_at: "2026-08-06T04:00:00.100Z",
    http_version: "HTTP/2",
    reason_phrase: "OK",
    headers: [{ name: "content-type", value_base64: btoa("text/event-stream") }],
  },
  result: {
    ended_at: completedSummary.ended_at,
    outcome: "completed",
    total_ms: 1250,
    error: null,
  },
  summary: {
    schema_version: 2,
    record_id: completedSummary.id,
    kind: "summary",
    observed_at: completedSummary.started_at,
    request: {
      method: completedSummary.method,
      incoming_uri: completedSummary.incoming_uri,
      upstream_url: completedSummary.upstream_url,
      http_version: "HTTP/2.0",
    },
    response: {
      status: 200,
      http_version: "HTTP/2",
    },
    terminal: true,
    timing: {
      upstream_request_started_at_ns: "100000000",
      upstream_request_body_first_byte_at_ns: "120000000",
      upstream_request_body_completed_at_ns: "200000000",
      upstream_response_headers_at_ns: "500000000",
      upstream_response_body_first_byte_at_ns: "520000000",
      upstream_response_body_completed_at_ns: "1230000000",
      finished_at_ns: "1250000000",
    },
    coding_agent_session_id: "629a8f94-d2cb-404c-9c10-a2a682478259",
    protocol: completedProtocol,
    outcome: "completed",
    errors: [],
    warnings: [],
    assessment: okAssessment,
  },
  assessment: okAssessment,
  diagnostics: {
    traffic: [],
    http: [],
    provider: [],
    warnings: [],
  },
  state: "completed",
  request_body_bytes: 7,
  response_body_bytes: 8,
  live_total_ms: null,
  timeline_end_at_ns: "1250000000",
} satisfies RecordDetail;

export const activeDetail = {
  ...completedDetail,
  request: {
    ...completedDetail.request,
    id: activeSummary.id,
    method: activeSummary.method,
    incoming_uri: activeSummary.incoming_uri,
    upstream_url: activeSummary.upstream_url,
  },
  response: null,
  result: null,
  summary: {
    ...completedDetail.summary,
    record_id: activeSummary.id,
    request: {
      ...completedDetail.summary.request,
      method: activeSummary.method,
      incoming_uri: activeSummary.incoming_uri,
      upstream_url: activeSummary.upstream_url,
    },
    response: null,
    terminal: false,
    timing: {
      ...completedDetail.summary.timing,
      upstream_response_headers_at_ns: null,
      upstream_response_body_first_byte_at_ns: null,
      upstream_response_body_completed_at_ns: null,
      finished_at_ns: null,
    },
    protocol: activeProtocol,
    outcome: null,
    assessment: activeAssessment,
  },
  assessment: activeAssessment,
  state: "active",
  request_body_bytes: 0,
  response_body_bytes: 0,
  live_total_ms: 500,
  timeline_end_at_ns: "500000000",
} satisfies RecordDetail;

export function withRequestEncoding(detail: RecordDetail, encoding: string): RecordDetail {
  return {
    ...detail,
    request: {
      ...detail.request,
      headers: [
        ...detail.request.headers,
        { name: "content-encoding", value_base64: btoa(encoding) },
      ],
    },
  };
}

export function withIncompleteRequestBody(detail: RecordDetail): RecordDetail {
  return {
    ...detail,
    summary: {
      ...detail.summary,
      timing: {
        ...detail.summary.timing,
        upstream_request_body_completed_at_ns: null,
      },
    },
  };
}

export function fakeApi(overrides: Partial<TrafficApi> = {}): TrafficApi {
  return {
    listRecords: vi.fn().mockResolvedValue(recordList),
    getRecord: vi.fn().mockResolvedValue(completedDetail),
    loadBody: vi.fn().mockResolvedValue({ bytes: new Uint8Array(), nextOffset: 0 }),
    loadDecodedBody: vi.fn().mockResolvedValue(new Uint8Array()),
    loadEventTimings: vi.fn().mockResolvedValue({
      state: "unavailable",
      events: [],
      next_sequence: 0,
      warning: null,
    }),
    deleteRecords: vi.fn().mockResolvedValue(0),
    deleteAll: vi.fn().mockResolvedValue(0),
    ...overrides,
  };
}
