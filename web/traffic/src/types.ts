export type RecordState = "active" | "completed" | "interrupted";

export interface HeaderValue {
  name: string;
  value_base64: string;
}

export interface RequestMetadata {
  id: string;
  started_at: string;
  method: string;
  incoming_uri: string;
  upstream_url: string | null;
  http_version: string;
  headers: HeaderValue[];
}

export interface ResponseMetadata {
  status: number;
  source: string;
  headers_at: string;
  headers: HeaderValue[];
}

export interface ResultMetadata {
  ended_at: string;
  outcome: string;
  ttfb_ms: number | null;
  total_ms: number | null;
}

export interface RecordSummary {
  id: string;
  started_at: string;
  method: string;
  incoming_uri: string;
  upstream_url: string | null;
  status: number | null;
  outcome: string;
  state: RecordState;
  total_ms: number | null;
}

export interface RecordList {
  records: RecordSummary[];
  total: number;
  deletable_count: number;
  next_cursor: string | null;
}

export interface RecordDetail {
  request: RequestMetadata;
  response: ResponseMetadata | null;
  result: ResultMetadata | null;
  state: RecordState;
  request_body_bytes: number;
  response_body_bytes: number;
  live_ttfb_ms: number | null;
  live_total_ms: number | null;
}

export interface TrafficApi {
  listRecords(cursor?: string, signal?: AbortSignal): Promise<RecordList>;
  getRecord(id: string, signal?: AbortSignal): Promise<RecordDetail>;
  loadBody(
    id: string,
    kind: "request" | "response",
    offset: number,
    signal?: AbortSignal,
  ): Promise<{ bytes: Uint8Array; nextOffset: number }>;
  deleteRecords(ids: string[], signal?: AbortSignal): Promise<number>;
  deleteAll(expectedDeletableCount: number, signal?: AbortSignal): Promise<number>;
}
