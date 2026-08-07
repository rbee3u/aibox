export type RecordState = "active" | "completed" | "interrupted";
export type BodyKind = "request" | "response";
export type BodyLoadStatus = "idle" | "loading" | "loaded" | "error";
export type DetailTab = "summary" | BodyKind;

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
  http_version: string;
  reason_phrase: string | null;
  headers: HeaderValue[];
}

export interface ErrorMetadata {
  kind: string;
  message: string;
}

export interface SummaryTiming {
  upstream_request_started_at_ns: string | null;
  upstream_request_body_first_byte_at_ns: string | null;
  upstream_request_body_completed_at_ns: string | null;
  upstream_response_headers_at_ns: string | null;
  upstream_response_body_first_byte_at_ns: string | null;
  upstream_response_body_completed_at_ns: string | null;
  finished_at_ns: string | null;
}

export interface SummaryDiagnostic {
  phase: string;
  kind: string;
  message: string;
  at_ns: string;
}

export interface SummaryMetadata {
  schema_version: number;
  record_id: string;
  kind: string;
  observed_at: string;
  terminal: boolean;
  timing: SummaryTiming;
  outcome: string | null;
  errors: SummaryDiagnostic[];
  warnings: SummaryDiagnostic[];
}

export interface ResultMetadata {
  ended_at: string;
  outcome: string;
  total_ms: number | null;
  error: ErrorMetadata | null;
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
  summary?: SummaryMetadata;
  state: RecordState;
  request_body_bytes: number;
  response_body_bytes: number;
  live_total_ms: number | null;
}

export interface TrafficApi {
  listRecords(cursor?: string, signal?: AbortSignal): Promise<RecordList>;
  getRecord(id: string, signal?: AbortSignal): Promise<RecordDetail>;
  loadBody(
    id: string,
    kind: BodyKind,
    offset: number,
    signal?: AbortSignal,
  ): Promise<{ bytes: Uint8Array; nextOffset: number }>;
  deleteRecords(ids: string[], signal?: AbortSignal): Promise<number>;
  deleteAll(expectedDeletableCount: number, signal?: AbortSignal): Promise<number>;
}
