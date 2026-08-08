import type { RecordDetail, RequestedEffective } from "./types";

export type TimingStageTone = "request" | "wait" | "model" | "finalize";
export type TimingStageStatus = "complete" | "ongoing" | "incomplete";

export interface TimingStage {
  label: string;
  tone: TimingStageTone;
  status: TimingStageStatus;
  startPercent: number;
  widthPercent: number;
  durationMs: number;
}

export type RequestedEffectiveSource = "effective" | "requested";

export interface ResolvedRequestedEffective {
  value: string | null;
  source: RequestedEffectiveSource | null;
}

export function resolveRequestedEffective(
  field: RequestedEffective<string> | null | undefined,
): ResolvedRequestedEffective {
  if (field?.effective != null) return { value: field.effective, source: "effective" };
  if (field?.requested != null) return { value: field.requested, source: "requested" };
  return { value: null, source: null };
}

function parseNs(value: string | null | undefined): bigint | null {
  if (value == null || !/^\d+$/.test(value)) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

function nsToMs(value: bigint): number {
  const whole = value / 1_000_000n;
  if (whole > BigInt(Number.MAX_SAFE_INTEGER)) return Number.MAX_SAFE_INTEGER;
  const remainder = value % 1_000_000n;
  return Number(whole) + Number(remainder) / 1_000_000;
}

function percent(value: bigint, total: bigint): number {
  if (total <= 0n) return 0;
  return Number((value * 10_000n) / total) / 100;
}

export function elapsedNsMs(value: string | null | undefined): number | null {
  const parsed = parseNs(value);
  return parsed === null ? null : nsToMs(parsed);
}

export function timingStages(detail: RecordDetail): TimingStage[] {
  const axisEnd = parseNs(detail.timeline_end_at_ns);
  if (axisEnd === null || axisEnd <= 0n) return [];
  const axis = axisEnd;

  const timing = detail.summary.timing;
  const requestStarted = parseNs(timing.upstream_request_started_at_ns);
  const requestCompleted = parseNs(timing.upstream_request_body_completed_at_ns);
  const responseHeaders = parseNs(timing.upstream_response_headers_at_ns);
  const responseCompleted = parseNs(timing.upstream_response_body_completed_at_ns);
  const finished = parseNs(timing.finished_at_ns);
  const protocol = detail.summary.protocol;
  const firstToken = parseNs(protocol?.first_token_at_ns);
  const active = detail.state === "active";
  const responseMode = protocol?.response_mode.requested ?? protocol?.response_mode.observed;
  const streaming = responseMode === "stream";
  const stages: TimingStage[] = [];
  let cursor = 0n;

  function add(
    label: string,
    tone: TimingStageTone,
    boundary: bigint | null,
  ): "advanced" | "ended" | "invalid" {
    if (boundary !== null) {
      if (boundary < cursor || boundary > axis) return "invalid";
      stages.push({
        label,
        tone,
        status: "complete",
        startPercent: percent(cursor, axis),
        widthPercent: percent(boundary - cursor, axis),
        durationMs: nsToMs(boundary - cursor),
      });
      cursor = boundary;
      return "advanced";
    }
    if (axis > cursor) {
      stages.push({
        label,
        tone,
        status: active ? "ongoing" : "incomplete",
        startPercent: percent(cursor, axis),
        widthPercent: percent(axis - cursor, axis),
        durationMs: nsToMs(axis - cursor),
      });
    }
    return "ended";
  }

  if (add("Proxy setup", "request", requestStarted) !== "advanced") return stages;
  if (add("Request upload", "request", requestCompleted) !== "advanced") return stages;
  if (add("Response wait", "wait", responseHeaders) !== "advanced") return stages;

  if (streaming && firstToken !== null) {
    if (add("First-token wait", "wait", firstToken) !== "advanced") return stages;
    if (add("Model output", "model", responseCompleted) !== "advanced") return stages;
  } else if (streaming && active) {
    add("First-token wait", "wait", null);
    return stages;
  } else if (add("Response body", "model", responseCompleted) !== "advanced") {
    return stages;
  }

  add("Finalization", "finalize", finished);
  return stages;
}

const TOKEN_FORMAT = new Intl.NumberFormat("en-US");

export function tokenCount(value: number): string {
  return TOKEN_FORMAT.format(value);
}
