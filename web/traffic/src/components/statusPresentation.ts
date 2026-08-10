import type { AssessmentPrimary, RecordAssessment, RecordState, ResponseMetadata } from "../types";

export type RecordStatusTone = "active" | "error" | "neutral" | "success" | "warning";

export interface StatusPresentationInput {
  status: number | null;
  state: RecordState;
  assessment: RecordAssessment;
}

export interface AssessmentPresentation {
  label: string;
  message: string;
  tone: "error" | "warning";
  additionalIssues: number;
}

export interface RecordStatusPresentation {
  label: string;
  tone: RecordStatusTone;
  issue: AssessmentPresentation | null;
  phase: "Streaming" | null;
}

export interface RecordHeadlinePresentation {
  statusText: string | null;
  tone: RecordStatusTone;
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
  request_recording_failed: "Request recording failed",
  response_incomplete: "Response incomplete",
  response_recording_failed: "Response recording failed",
  server_shutdown: "Server shutdown",
  upgrade_not_supported: "Upgrade unsupported",
  upstream_request_failed: "Upstream request failed",
  upstream_response_failed: "Upstream stream failed",
};

function humanize(value: string, fallback: string): string {
  const words = value.trim().replace(/[_-]+/g, " ").replace(/\s+/g, " ").toLowerCase();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : fallback;
}

export function errorKindLabel(kind: string): string {
  return ERROR_KIND_LABELS[kind] ?? humanize(kind, "Unknown issue");
}

export function assessmentPrimaryLabel(primary: AssessmentPrimary): string {
  const httpStatus = primary.source === "http" ? /^http_(\d+)$/.exec(primary.kind)?.[1] : null;
  return httpStatus ? `HTTP ${httpStatus}` : errorKindLabel(primary.kind);
}

export function statusTone(status: number): RecordStatusTone {
  if (status >= 200 && status <= 299) return "success";
  if ((status >= 100 && status <= 199) || (status >= 300 && status <= 399)) {
    return "neutral";
  }
  return "error";
}

export function assessmentPresentation(
  assessment: RecordAssessment,
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
  return `Record ${issue.tone}: ${issue.label}. ${issue.message}`;
}

export function recordStatusPresentation({
  status,
  state,
  assessment,
}: StatusPresentationInput): RecordStatusPresentation {
  const active = state === "active";
  if (status === null) {
    return {
      label: active ? "Waiting" : "No response",
      tone: active ? "active" : "neutral",
      issue: active ? null : assessmentPresentation(assessment),
      phase: null,
    };
  }

  return {
    label: String(status),
    tone: statusTone(status),
    issue: active ? null : assessmentPresentation(assessment),
    phase: active ? "Streaming" : null,
  };
}

export function recordHeadlinePresentation(
  response: ResponseMetadata | null,
  state: RecordState,
  assessment: RecordAssessment,
): RecordHeadlinePresentation {
  const active = state === "active";
  const statusText = response
    ? [response.http_version, response.status, response.reason_phrase].filter(Boolean).join(" ")
    : active
      ? null
      : "No response";

  return {
    statusText,
    tone: response ? statusTone(response.status) : active ? "active" : "neutral",
    tag: active
      ? { label: response ? "Streaming" : "Waiting", tone: "active" }
      : assessmentPresentation(assessment),
  };
}
