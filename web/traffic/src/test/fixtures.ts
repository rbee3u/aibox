import { vi } from "vitest";
import type { RecordDetail, RecordList, RecordSummary, TrafficApi } from "../types";

export const completedSummary: RecordSummary = {
  id: "0198-demo-completed",
  started_at: "2026-08-06T04:00:00Z",
  method: "POST",
  incoming_uri: "/https://api.example.test/v1/responses?stream=true",
  upstream_url: "https://api.example.test/v1/responses?stream=true",
  status: 200,
  outcome: "completed",
  state: "completed",
  total_ms: 1250,
};

export const activeSummary: RecordSummary = {
  id: "0198-demo-active",
  started_at: "2026-08-06T04:01:00Z",
  method: "GET",
  incoming_uri: "/https://stream.example.test/events",
  upstream_url: "https://stream.example.test/events",
  status: null,
  outcome: "active",
  state: "active",
  total_ms: null,
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
    headers: [{ name: "content-type", value_base64: btoa("text/event-stream") }],
  },
  result: {
    ended_at: "2026-08-06T04:00:01.250Z",
    outcome: "completed",
    ttfb_ms: 100,
    total_ms: 1250,
  },
  state: "completed",
  request_body_bytes: 7,
  response_body_bytes: 8,
  live_ttfb_ms: null,
  live_total_ms: null,
};

export const activeDetail: RecordDetail = {
  ...completedDetail,
  request: { ...completedDetail.request, id: activeSummary.id },
  response: null,
  result: null,
  state: "active",
  request_body_bytes: 0,
  response_body_bytes: 0,
  live_ttfb_ms: null,
  live_total_ms: 500,
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
