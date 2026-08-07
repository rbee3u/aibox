import type { RecordDetail, RecordState, ResponseMetadata, ResultMetadata } from "../types";

export type RecordStatusTone = "active" | "error" | "neutral" | "success";

export interface StatusPresentationInput {
  status: number | null;
  outcome: string;
  state: RecordState;
}

export interface RecordStatusPresentation {
  label: string;
  tone: RecordStatusTone;
  anomaly: string | null;
  phase: "Streaming" | null;
}

export interface RecordErrorPresentation {
  label: string;
  message: string;
}

export interface RecordHeadlinePresentation {
  statusText: string | null;
  tone: RecordStatusTone;
  tag: string | null;
  tagTone: "active" | "error" | null;
}

const OUTCOME_LABELS: Record<string, string> = {
  rejected: "Rejected",
  upstream_error: "Upstream error",
  client_disconnected: "Client disconnected",
  recording_failed: "Recording failed",
  server_shutdown: "Server shutdown",
  interrupted: "Interrupted",
};

const ERROR_KIND_LABELS: Record<string, string> = {
  client_configuration: "HTTP client setup failed",
  client_disconnected: "Client disconnected",
  connect_not_supported: "CONNECT unsupported",
  connect_timeout: "Connect timeout",
  dns_error: "DNS failed",
  invalid_target_url: "Invalid target",
  non_public_target: "Target blocked",
  recording_failed: "Recording failed",
  request_body_failed: "Request body failed",
  request_recording_failed: "Request recording failed",
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

export function outcomeLabel(outcome: string): string {
  return OUTCOME_LABELS[outcome] ?? humanize(outcome, "Unknown error");
}

export function errorKindLabel(kind: string): string {
  return ERROR_KIND_LABELS[kind] ?? humanize(kind, "Unknown error");
}

export function statusTone(status: number): RecordStatusTone {
  if (status >= 200 && status <= 299) return "success";
  if ((status >= 100 && status <= 199) || (status >= 300 && status <= 399)) {
    return "neutral";
  }
  return "error";
}

export function recordStatusPresentation({
  status,
  outcome,
  state,
}: StatusPresentationInput): RecordStatusPresentation {
  const active = state === "active";
  const anomaly = !active && outcome !== "completed" ? outcomeLabel(outcome) : null;

  if (status === null) {
    return {
      label: active ? "Waiting" : "No response",
      tone: active ? "active" : "error",
      anomaly,
      phase: null,
    };
  }

  return {
    label: String(status),
    tone: statusTone(status),
    anomaly,
    phase: active ? "Streaming" : null,
  };
}

export function recordErrorPresentation(
  detail: Pick<RecordDetail, "result" | "state">,
): RecordErrorPresentation | null {
  if (detail.state === "active") return null;
  if (detail.state === "interrupted") {
    return {
      label: "Interrupted",
      message: "Traffic Proxy stopped before the Traffic Record was finalized.",
    };
  }

  const result = detail.result;
  if (!result || result.outcome === "completed") return null;
  return {
    label: result.error ? errorKindLabel(result.error.kind) : outcomeLabel(result.outcome),
    message: result.error?.message ?? "No additional error details were recorded.",
  };
}

export function recordHeadlinePresentation(
  response: ResponseMetadata | null,
  result: ResultMetadata | null,
  state: RecordState,
): RecordHeadlinePresentation {
  const active = state === "active";
  const error = recordErrorPresentation({ result, state });
  const statusText = response
    ? [response.http_version, response.status, response.reason_phrase].filter(Boolean).join(" ")
    : active
      ? null
      : "No response";

  return {
    statusText,
    tone: response ? statusTone(response.status) : active ? "active" : "error",
    tag: active ? (response ? "Streaming" : "Waiting") : (error?.label ?? null),
    tagTone: active ? "active" : error ? "error" : null,
  };
}
