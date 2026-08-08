import { vi } from "vitest";
import type {
  ProtocolSummary,
  RecordDetail,
  RecordList,
  RecordSummary,
  TrafficApi,
} from "../types";

export const completedProtocol: ProtocolSummary = {
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
};

export const activeProtocol: ProtocolSummary = {
  ...completedProtocol,
  response_terminal: false,
  model: { requested: "gpt-5.6-sol", effective: null },
  reasoning_effort: { requested: "high", effective: null },
  first_token_at_ns: null,
  token_usage: null,
};

export const completedSummary: RecordSummary = {
  id: "0198-demo-completed",
  started_at: "2026-08-06T04:00:00Z",
  method: "POST",
  incoming_uri: "/https://api.example.test/v1/responses?stream=true",
  upstream_url: "https://api.example.test/v1/responses?stream=true",
  status: 200,
  http_version: "HTTP/2",
  outcome: "completed",
  state: "completed",
  total_ms: 1250,
  protocol: completedProtocol,
};

export const activeSummary: RecordSummary = {
  id: "0198-demo-active",
  started_at: "2026-08-06T04:01:00Z",
  method: "GET",
  incoming_uri: "/https://stream.example.test/events",
  upstream_url: "https://stream.example.test/events",
  status: null,
  http_version: null,
  outcome: "active",
  state: "active",
  total_ms: 500,
  protocol: activeProtocol,
};

export const recordList: RecordList = {
  records: [activeSummary, completedSummary],
  total: 2,
  deletable_count: 1,
  next_cursor: null,
};

export const completedDetail: RecordDetail = {
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
    ended_at: "2026-08-06T04:00:01.250Z",
    outcome: "completed",
    total_ms: 1250,
    error: null,
  },
  summary: {
    schema_version: 1,
    record_id: completedSummary.id,
    kind: "summary",
    observed_at: completedSummary.started_at,
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
    protocol: completedProtocol,
    outcome: "completed",
    errors: [],
    warnings: [],
  },
  state: "completed",
  request_body_bytes: 7,
  response_body_bytes: 8,
  live_total_ms: null,
  timeline_end_at_ns: "1250000000",
};

export const activeDetail: RecordDetail = {
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
  },
  state: "active",
  request_body_bytes: 0,
  response_body_bytes: 0,
  live_total_ms: 500,
  timeline_end_at_ns: "500000000",
};

export function fakeApi(overrides: Partial<TrafficApi> = {}): TrafficApi {
  return {
    listRecords: vi.fn().mockResolvedValue(recordList),
    getRecord: vi.fn().mockResolvedValue(completedDetail),
    loadBody: vi.fn().mockResolvedValue({ bytes: new Uint8Array(), nextOffset: 0 }),
    deleteRecords: vi.fn().mockResolvedValue(0),
    deleteAll: vi.fn().mockResolvedValue(0),
    ...overrides,
  };
}
