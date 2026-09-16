import type {
  AssessmentPrimary,
  RequestAssessment,
  RequestState,
  ResponseMetadata,
} from "@/api/requests";

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

/**
 * Catalog status cell. The cell carries the Assessment: a finding that adds to
 * an HTTP status hangs a level marker after the code — an error also turns the
 * whole cell red, a warning leaves the code its own tone — and a finding on a
 * Request that never got a status becomes the label itself. `issue` is the
 * reason the cell explains on hover; it is null when the status already says
 * everything.
 */
interface RequestStatusPresentation {
  label: string;
  tone: RequestStatusTone;
  issue: AssessmentPresentation | null;
  marker: "error" | "warning" | null;
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
  client_configuration: "Client setup failed",
  client_disconnected: "Disconnected",
  connect_not_supported: "CONNECT unsupported",
  connect_timeout: "Connect timeout",
  dns_error: "DNS failed",
  event_index_failed: "Timing unavailable",
  failed: "Response failed",
  invalid_target_url: "Invalid target",
  model_response_terminal_not_observed: "Terminal missing",
  non_public_target: "Target blocked",
  recording_failed: "Recording failed",
  request_body_failed: "Request body failed",
  request_recording_failed: "Requesting failed",
  response_incomplete: "Response incomplete",
  response_recording_failed: "Recording failed",
  server_shutdown: "Server shutdown",
  upgrade_not_supported: "Upgrade unsupported",
  upstream_request_failed: "Upstream failed",
  upstream_response_failed: "Stream failed",
};

export function errorKindLabel(kind: string): string {
  const known = ERROR_KIND_LABELS[kind];
  if (known) return known;
  const words = kind.trim().replace(/[_-]+/g, " ").replace(/\s+/g, " ").toLowerCase();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : "Unknown issue";
}

export function assessmentPrimaryLabel(primary: AssessmentPrimary): string {
  const httpStatus = httpStatusFromPrimary(primary);
  return httpStatus != null ? `HTTP ${httpStatus}` : errorKindLabel(primary.kind);
}

/** True when the Assessment primary is the same HTTP status already on the record. */
export function assessmentRestatesHttpStatus(
  assessment: RequestAssessment,
  status: number | null,
): boolean {
  if (status == null || !assessment.primary) return false;
  return httpStatusFromPrimary(assessment.primary) === status;
}

function httpStatusFromPrimary(primary: AssessmentPrimary): number | null {
  if (primary.source !== "http") return null;
  const match = /^http_(\d+)$/.exec(primary.kind);
  if (!match) return null;
  const status = Number(match[1]);
  return Number.isInteger(status) ? status : null;
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
  if (state === "active") {
    return status === null
      ? { label: "Waiting", tone: "active", issue: null, marker: null, phase: null }
      : {
          label: String(status),
          tone: statusTone(status),
          issue: null,
          marker: null,
          phase: "Streaming",
        };
  }

  const issue = assessmentPresentation(assessment);
  if (status === null) {
    return issue
      ? { label: issue.label, tone: issue.tone, issue, marker: null, phase: null }
      : { label: "No response", tone: "neutral", issue: null, marker: null, phase: null };
  }

  const tone = statusTone(status);
  if (!issue || assessmentRestatesHttpStatus(assessment, status)) {
    return { label: String(status), tone, issue: null, marker: null, phase: null };
  }
  return {
    label: String(status),
    tone: issue.tone === "error" ? "error" : tone,
    issue,
    marker: issue.tone,
    phase: null,
  };
}

export function requestHeadlinePresentation(
  response: ResponseMetadata | null,
  state: RequestState,
  assessment: RequestAssessment,
): RecordHeadlinePresentation {
  const active = state === "active";
  if (!response) {
    if (active)
      return { statusText: null, tone: "active", tag: { label: "Waiting", tone: "active" } };
    // The failure kind is the status: the list already states it alone in
    // the status column (finding 51), and "No response" beside it said the
    // same thing twice with the grey half first.
    const issue = assessmentPresentation(assessment);
    return issue
      ? { statusText: null, tone: issue.tone, tag: issue }
      : { statusText: "No response", tone: "neutral", tag: null };
  }
  return {
    statusText: [response.http_version, response.status, response.reason_phrase]
      .filter(Boolean)
      .join(" "),
    tone: statusTone(response.status),
    tag: active
      ? { label: "Streaming", tone: "active" }
      : assessmentRestatesHttpStatus(assessment, response.status)
        ? null
        : assessmentPresentation(assessment),
  };
}
