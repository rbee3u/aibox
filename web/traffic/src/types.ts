export type RecordState = "active" | "completed" | "interrupted";
export type BodyKind = "request" | "response";
export type BodyLoadStatus = "idle" | "loaded" | "error";
export type DetailTab = "summary" | BodyKind;
type EventTimingState = "available" | "unavailable" | "partial";
type AssessmentLevel = "active" | "ok" | "warning" | "error";
type AssessmentSource = "traffic" | "http" | "provider" | "diagnostic";

interface EventTimingEntry {
  sequence: number;
  completed_at_ns: string;
}

export interface EventTimingIndex {
  state: EventTimingState;
  events: EventTimingEntry[];
  next_sequence: number;
  warning: string | null;
}

export interface DecodedBodyState {
  bytes: Uint8Array | null;
  error: string | null;
}

export interface HeaderValue {
  name: string;
  value_base64: string;
}

interface RequestMetadata {
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

interface ErrorMetadata {
  kind: string;
  message: string;
}

interface SummaryTiming {
  upstream_request_started_at_ns: string | null;
  upstream_request_body_first_byte_at_ns: string | null;
  upstream_request_body_completed_at_ns: string | null;
  upstream_response_headers_at_ns: string | null;
  upstream_response_body_first_byte_at_ns: string | null;
  upstream_response_body_completed_at_ns: string | null;
  finished_at_ns: string | null;
}

interface SummaryDiagnostic {
  phase: string;
  kind: string;
  message: string;
  at_ns: string;
}

interface SummaryRequestMetadata {
  method: string;
  incoming_uri: string;
  upstream_url: string | null;
  http_version: string;
}

interface SummaryResponseMetadata {
  status: number;
  http_version: string;
}

export interface AssessmentPrimary {
  source: AssessmentSource;
  kind: string;
  message: string;
}

export interface RecordAssessment {
  level: AssessmentLevel;
  primary: AssessmentPrimary | null;
  issue_count: number;
}

export interface AssessmentFinding extends AssessmentPrimary {
  level: AssessmentLevel;
  phase: string | null;
  at_ns: string | null;
}

type DiagnosticGroups = Record<"traffic" | "http" | "provider" | "warnings", AssessmentFinding[]>;

interface SummaryMetadata {
  schema_version: number;
  record_id: string;
  kind: string;
  observed_at: string;
  request: SummaryRequestMetadata;
  response: SummaryResponseMetadata | null;
  terminal: boolean;
  timing: SummaryTiming;
  coding_agent_session_id: string | null;
  protocol: ProtocolSummary | null;
  outcome: string | null;
  errors: SummaryDiagnostic[];
  warnings: SummaryDiagnostic[];
  assessment: RecordAssessment;
}

type ProtocolFamily = "openai_responses" | "claude_messages" | "unknown";
export type ResponseModeValue = "stream" | "normal";

export interface RequestedEffective<T> {
  requested: T | null;
  effective: T | null;
}

interface RequestedObserved<T> {
  requested: T | null;
  observed: T | null;
}

export interface TokenUsage {
  total_input_tokens: number | null;
  base_input_tokens: number | null;
  cached_input_tokens: number | null;
  cache_write_tokens: number | null;
  cache_write_5m_tokens: number | null;
  cache_write_1h_tokens: number | null;
  output_tokens: number | null;
  reasoning_output_tokens: number | null;
}

interface ProtocolDiagnostic {
  kind: string;
  message: string;
  at_ns: string | null;
}

export interface ProtocolSummary {
  family: ProtocolFamily;
  response_terminal: boolean;
  model: RequestedEffective<string>;
  reasoning_effort: RequestedEffective<string>;
  response_mode: RequestedObserved<ResponseModeValue>;
  first_token_at_ns: string | null;
  token_usage: TokenUsage | null;
  errors: ProtocolDiagnostic[];
  warnings: ProtocolDiagnostic[];
}

interface ResultMetadata {
  ended_at: string;
  outcome: string;
  total_ms: number | null;
  error: ErrorMetadata | null;
}

export interface RecordSummary {
  id: string;
  started_at: string;
  ended_at: string | null;
  method: string;
  incoming_uri: string;
  upstream_url: string | null;
  status: number | null;
  http_version: string | null;
  outcome: string;
  state: RecordState;
  total_ms: number | null;
  protocol: ProtocolSummary | null;
  assessment: RecordAssessment;
}

export interface RecordList {
  records: RecordSummary[];
  total: number;
  deletable_count: number;
  has_next: boolean;
}

export interface RecordDetail {
  request: RequestMetadata;
  response: ResponseMetadata | null;
  result: ResultMetadata | null;
  summary: SummaryMetadata;
  assessment: RecordAssessment;
  diagnostics: DiagnosticGroups;
  state: RecordState;
  request_body_bytes: number;
  response_body_bytes: number;
  live_total_ms: number | null;
  timeline_end_at_ns: string | null;
}

export interface TrafficApi {
  listRecords(page?: number, signal?: AbortSignal): Promise<RecordList>;
  getRecord(id: string, signal?: AbortSignal): Promise<RecordDetail>;
  loadBody(
    id: string,
    kind: BodyKind,
    offset: number,
    signal?: AbortSignal,
  ): Promise<{ bytes: Uint8Array; nextOffset: number }>;
  loadDecodedBody(id: string, kind: BodyKind, signal?: AbortSignal): Promise<Uint8Array>;
  loadEventTimings(
    id: string,
    afterSequence: number,
    signal?: AbortSignal,
  ): Promise<EventTimingIndex>;
  deleteRecords(ids: string[], signal?: AbortSignal): Promise<number>;
  deleteAll(signal?: AbortSignal): Promise<number>;
}
