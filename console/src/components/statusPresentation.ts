import type {
  AssessmentPrimary,
  RequestAssessment,
  RequestState,
  ResponseMetadata,
} from "../types";

export type RequestStatusTone = "active" | "error" | "neutral" | "success" | "warning";

interface StatusPresentationInput {
  status: number | null;
  state: RequestState;
  assessment: RequestAssessment;
}

export interface AssessmentPresentation {
  label: string;
  message: string;
  tone: "error" | "warning";
  additionalIssues: number;
}

interface RequestStatusPresentation {
  label: string;
  tone: RequestStatusTone;
  issue: AssessmentPresentation | null;
  phase: "Streaming" | null;
}

interface RecordHeadlinePresentation {
  statusText: string | null;
  tone: RequestStatusTone;
  tag: AssessmentPresentation | { label: "Waiting" | "Streaming"; tone: "active" } | null;
}

const ERROR_KIND_LABELS: Record<string, string> = {
  api_error: "Model API error",
  cancelled: "Response cancelled",
  client_configuration: "HTTP client setup failed",
  client_disconnected: "Client disconnected",
  connect_not_supported: "CONNECT unsupported",
  connect_timeout: "Connect timeout",
  dns_error: "DNS failed",
  event_index_failed: "Event timing unavailable",
  failed: "Response failed",
  invalid_target_url: "Invalid target",
  model_response_terminal_not_observed: "Terminal event missing",
  non_public_target: "Target blocked",
  recording_failed: "Recording failed",
  request_body_failed: "Request body failed",
  request_recording_failed: "Requesting failed",
  response_incomplete: "Response incomplete",
  response_recording_failed: "Response recording failed",
  server_shutdown: "Server shutdown",
  upgrade_not_supported: "Upgrade unsupported",
  upstream_request_failed: "Upstream request failed",
  upstream_response_failed: "Upstream stream failed",
};

export function errorKindLabel(kind: string): string {
  const known = ERROR_KIND_LABELS[kind];
  if (known) return known;
  const words = kind.trim().replace(/[_-]+/g, " ").replace(/\s+/g, " ").toLowerCase();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : "Unknown issue";
}

export function assessmentPrimaryLabel(primary: AssessmentPrimary): string {
  const httpStatus = primary.source === "http" ? /^http_(\d+)$/.exec(primary.kind)?.[1] : null;
  return httpStatus ? `HTTP ${httpStatus}` : errorKindLabel(primary.kind);
}

export function statusTone(status: number): RequestStatusTone {
  if (status >= 200 && status < 300) return "success";
  if (status >= 100 && status < 400) return "neutral";
  return "error";
}

export function assessmentPresentation(
  assessment: RequestAssessment,
): AssessmentPresentation | null {
  if ((assessment.level !== "error" && assessment.level !== "warning") || !assessment.primary) {
    return null;
  }
  return {
    label: assessmentPrimaryLabel(assessment.primary),
    message: assessment.primary.message,
    tone: assessment.level,
    additionalIssues: Math.max(0, assessment.issue_count - 1),
  };
}

export function assessmentIssueText(issue: AssessmentPresentation): string {
  return `Request ${issue.tone}: ${issue.label}. ${issue.message}`;
}

export function requestStatusPresentation({
  status,
  state,
  assessment,
}: StatusPresentationInput): RequestStatusPresentation {
  const active = state === "active";
  const issue = active ? null : assessmentPresentation(assessment);
  if (status === null) {
    return {
      label: active ? "Waiting" : "No response",
      tone: active ? "active" : "neutral",
      issue,
      phase: null,
    };
  }

  return {
    label: String(status),
    tone: statusTone(status),
    issue,
    phase: active ? "Streaming" : null,
  };
}

export function requestHeadlinePresentation(
  response: ResponseMetadata | null,
  state: RequestState,
  assessment: RequestAssessment,
): RecordHeadlinePresentation {
  const active = state === "active";
  if (!response) {
    return {
      statusText: active ? null : "No response",
      tone: active ? "active" : "neutral",
      tag: active ? { label: "Waiting", tone: "active" } : assessmentPresentation(assessment),
    };
  }
  return {
    statusText: [response.http_version, response.status, response.reason_phrase]
      .filter(Boolean)
      .join(" "),
    tone: statusTone(response.status),
    tag: active ? { label: "Streaming", tone: "active" } : assessmentPresentation(assessment),
  };
}
