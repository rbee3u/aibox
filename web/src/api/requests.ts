import { HttpError } from "@/api/httpError";
import type { ControlApi } from "@/api/transport";
import type {
  AssessmentFinding,
  AssessmentLevel,
  AssessmentPrimary,
  AssessmentSource,
  EventTimingEntry,
  EventTimingResponse,
  EventTimingState,
  ProtocolDiagnostic,
  ProtocolFamily,
  ProtocolSummary,
  RecordedHeader,
  RequestAssessment,
  RequestDetail as GeneratedRequestDetail,
  RequestList,
  RequestMetadata as GeneratedRequestMetadata,
  RequestState,
  RequestSummary,
  RequestedEffective,
  RequestedObserved,
  ResponseDetail as GeneratedResponseDetail,
  ResponseModeValue,
  ResultMetadata as GeneratedResultMetadata,
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
  RequestList,
  RequestedEffective,
  RequestedObserved,
  ResponseModeValue,
  TokenUsage,
};
export type HeaderValue = RecordedHeader;
export type BodyKind = "request" | "response";

export type EventTimingIndex = EventTimingResponse;

export type RequestMetadata = Omit<GeneratedRequestMetadata, "format_version">;
export type ResponseMetadata = Omit<GeneratedResponseDetail, "format_version">;
type ResultMetadata = Omit<
  GeneratedResultMetadata,
  "format_version" | "request_bytes" | "response_bytes" | "request_body_ms"
>;

export type RequestDetail = Omit<GeneratedRequestDetail, "request" | "response" | "result"> & {
  request: RequestMetadata;
  response: ResponseMetadata | null;
  result: ResultMetadata | null;
};

export type RequestLookup = RequestDetail | { kind: "missing" };

export function isRequestNotFound(cause: unknown): boolean {
  return cause instanceof HttpError && cause.status === 404;
}

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
      return client.get<RequestList>(`/_aibox/api/requests${query}`, signal);
    },
    getRequest: async (id, signal) => {
      try {
        const response = await client.get<GeneratedRequestDetail>(requestPath(id), signal);
        return featureRequestDetail(response);
      } catch (cause) {
        if (isRequestNotFound(cause)) return { kind: "missing" };
        throw cause;
      }
    },
    loadBody: async (id, kind, offset, signal) => {
      const response = await client.getResponse(
        `${requestPath(id)}/${kind}-body?offset=${offset}`,
        signal,
      );
      const bytes = new Uint8Array(await response.arrayBuffer());
      // Advance only by received bytes, regardless of the advertised offset.
      return { bytes, nextOffset: offset + bytes.length };
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
    deleteRequests: async (ids, signal) => {
      const response = await client.post<{ deleted: number }>(
        "/_aibox/api/requests/delete",
        { ids },
        signal,
      );
      return response.deleted;
    },
  };
}
