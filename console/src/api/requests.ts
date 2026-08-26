import type { ControlApi } from "@/api/transport";

export type RequestState = "active" | "completed" | "interrupted";
export type BodyKind = "request" | "response";
type EventTimingState = "available" | "unavailable" | "partial";
type AssessmentLevel = "active" | "ok" | "warning" | "error";
type AssessmentSource = "request" | "http" | "provider" | "diagnostic";

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

export interface RequestAssessment {
  level: AssessmentLevel;
  primary: AssessmentPrimary | null;
  issue_count: number;
}

export interface AssessmentFinding extends AssessmentPrimary {
  level: AssessmentLevel;
  phase: string | null;
  at_ns: string | null;
}

type DiagnosticGroups = Record<"request" | "http" | "provider" | "warnings", AssessmentFinding[]>;

interface SummaryMetadata {
  schema_version: number;
  request_id: string;
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
  assessment: RequestAssessment;
}

type ProtocolFamily =
  "openai_responses" | "openai_chat_completions" | "claude_messages" | "unknown";

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

export interface RequestSummary {
  id: string;
  started_at: string;
  ended_at: string | null;
  method: string;
  incoming_uri: string;
  upstream_url: string | null;
  status: number | null;
  http_version: string | null;
  outcome: string;
  state: RequestState;
  total_ms: number | null;
  protocol: ProtocolSummary | null;
  assessment: RequestAssessment;
}

export interface RequestList {
  requests: RequestSummary[];
  total: number;
  deletable_count: number;
  has_next: boolean;
}

export interface RequestDetail {
  request: RequestMetadata;
  response: ResponseMetadata | null;
  result: ResultMetadata | null;
  summary: SummaryMetadata;
  assessment: RequestAssessment;
  diagnostics: DiagnosticGroups;
  state: RequestState;
  request_body_bytes: number;
  response_body_bytes: number;
  live_total_ms: number | null;
  timeline_end_at_ns: string | null;
}

export interface RequestsApi {
  listRequests(page?: number, signal?: AbortSignal): Promise<RequestList>;
  getRequest(id: string, signal?: AbortSignal): Promise<RequestDetail>;
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
  deleteRequests(ids: string[], signal?: AbortSignal): Promise<number>;
}

function requestPath(id: string) {
  return `/_aibox/api/requests/${encodeURIComponent(id)}`;
}

export function requestsApi(client: ControlApi): RequestsApi {
  return {
    listRequests: (page = 1, signal) => {
      const query = page === 1 ? "" : `?page=${page}`;
      return client.get<RequestList>(`/_aibox/api/requests${query}`, signal);
    },
    getRequest: (id, signal) => client.get<RequestDetail>(requestPath(id), signal),
    loadBody: async (id, kind, offset, signal) => {
      const response = await client.getResponse(
        `${requestPath(id)}/${kind}-body?offset=${offset}`,
        signal,
      );
      const bytes = new Uint8Array(await response.arrayBuffer());
      const header = response.headers.get("X-Aibox-Request-Next-Offset");
      const fallbackOffset = offset + bytes.length;
      const advertisedOffset = header === null ? null : Number(header);
      const nextOffset =
        advertisedOffset !== null &&
        Number.isSafeInteger(advertisedOffset) &&
        advertisedOffset === fallbackOffset
          ? advertisedOffset
          : fallbackOffset;
      return { bytes, nextOffset };
    },
    loadDecodedBody: async (id, kind, signal) => {
      const response = await client.getResponse(`${requestPath(id)}/${kind}-body-decoded`, signal);
      return new Uint8Array(await response.arrayBuffer());
    },
    loadEventTimings: (id, afterSequence, signal) =>
      client.get<EventTimingIndex>(
        `${requestPath(id)}/response-event-timings?after_sequence=${afterSequence}`,
        signal,
      ),
    deleteRequests: (ids, signal) =>
      client
        .post<{ deleted: number }>("/_aibox/api/requests/delete", { ids }, signal)
        .then((value) => value.deleted),
  };
}
