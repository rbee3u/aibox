import { HttpError as ApiError } from "@/api/httpError";
import type { ControlApi } from "@/api/transport";
export { ApiError };
import type {
  AssessmentFinding,
  AssessmentLevel,
  AssessmentPrimary,
  AssessmentSource,
  DiagnosticGroups as GeneratedDiagnosticGroups,
  EventTimingEntry,
  EventTimingResponse,
  EventTimingState,
  ProtocolDiagnostic,
  ProtocolFamily,
  ProtocolSummary,
  RecordedHeader,
  RequestAssessment,
  RequestDetail as GeneratedRequestDetail,
  RequestList as GeneratedRequestList,
  RequestMetadata as GeneratedRequestMetadata,
  RequestState,
  RequestSummary,
  RequestedEffective,
  RequestedObserved,
  ResponseModeValue,
  ResultMetadata as GeneratedResultMetadata,
  SummaryMetadata as GeneratedSummaryMetadata,
  TokenUsage,
} from "@/api/generated/wire";

export type {
  AssessmentFinding,
  AssessmentLevel,
  AssessmentPrimary,
  AssessmentSource,
  EventTimingEntry,
  EventTimingState,
  ProtocolDiagnostic,
  ProtocolFamily,
  ProtocolSummary,
  RequestAssessment,
  RequestState,
  RequestSummary,
  RequestedEffective,
  RequestedObserved,
  ResponseModeValue,
  TokenUsage,
};
export type HeaderValue = RecordedHeader;
export type BodyKind = "request" | "response";

export type EventTimingIndex = EventTimingResponse;

export type RequestMetadata = Omit<GeneratedRequestMetadata, "format_version">;

export interface ResponseMetadata {
  status: number;
  source: string;
  headers_at: string;
  http_version: string;
  reason_phrase: string | null;
  headers: HeaderValue[];
}

type DiagnosticGroups = GeneratedDiagnosticGroups;
type SummaryMetadata = Omit<GeneratedSummaryMetadata, "outcome"> & { outcome: string | null };
type ResultMetadata = Omit<
  GeneratedResultMetadata,
  "format_version" | "request_bytes" | "response_bytes" | "request_body_ms"
>;
export type RequestList = GeneratedRequestList;

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

export type RequestLookup = RequestDetail | { kind: "missing" };

function featureRequestDetail(value: GeneratedRequestDetail): RequestDetail {
  if (!value || typeof value !== "object" || !value.request) {
    return value;
  }
  const request = withoutFormatVersion(value.request);
  const response = value.response ? withoutFormatVersion(value.response) : null;
  const result = value.result ? featureResult(value.result) : null;
  return { ...value, request, response, result };
}

function withoutFormatVersion<T extends { format_version: number }>(
  value: T,
): Omit<T, "format_version"> {
  const { format_version: formatVersion, ...copy } = value;
  void formatVersion;
  return copy;
}

function featureResult(value: GeneratedResultMetadata): ResultMetadata {
  const {
    format_version: formatVersion,
    request_body_ms: requestBodyMs,
    request_bytes: requestBytes,
    response_bytes: responseBytes,
    ...copy
  } = value;
  void formatVersion;
  void requestBodyMs;
  void requestBytes;
  void responseBytes;
  return copy;
}

export interface RequestsApi {
  listRequests(page?: number, signal?: AbortSignal): Promise<RequestList>;
  getRequest(id: string, signal?: AbortSignal): Promise<RequestLookup>;
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
      return client.get<GeneratedRequestList>(`/_aibox/api/requests${query}`, signal);
    },
    getRequest: (id, signal) =>
      client
        .get<GeneratedRequestDetail>(requestPath(id), signal)
        .then(featureRequestDetail)
        .catch((cause: unknown) => {
          if (cause instanceof ApiError && cause.status === 404)
            return { kind: "missing" } as const;
          throw cause;
        }),
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
      client.get<EventTimingResponse>(
        `${requestPath(id)}/response-event-timings?after_sequence=${afterSequence}`,
        signal,
      ),
    deleteRequests: (ids, signal) =>
      client
        .post<{ deleted: number }>("/_aibox/api/requests/delete", { ids }, signal)
        .then((value) => value.deleted),
  };
}
