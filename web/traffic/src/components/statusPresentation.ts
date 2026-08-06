import type { RecordState } from "../types";

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
  recording: boolean;
}

const OUTCOME_LABELS: Record<string, string> = {
  rejected: "Rejected",
  upstream_error: "Upstream error",
  client_disconnected: "Client disconnected",
  recording_failed: "Recording failed",
  server_shutdown: "Server shutdown",
  interrupted: "Interrupted",
};

export function outcomeLabel(outcome: string): string {
  const known = OUTCOME_LABELS[outcome];
  if (known) return known;

  const words = outcome.trim().replace(/[_-]+/g, " ").replace(/\s+/g, " ").toLowerCase();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : "Unknown error";
}

function statusTone(status: number): RecordStatusTone {
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
      label: active ? "Active" : "No response",
      tone: active ? "active" : "error",
      anomaly,
      recording: false,
    };
  }

  return {
    label: String(status),
    tone: statusTone(status),
    anomaly,
    recording: active,
  };
}
