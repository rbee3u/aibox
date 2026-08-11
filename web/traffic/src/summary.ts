import type { RecordDetail, RequestedEffective } from "./types";

type TimingStageTone = "request" | "wait" | "model" | "finalize";
type TimingStageStatus = "complete" | "ongoing" | "incomplete";

interface TimingStage {
  label: string;
  tone: TimingStageTone;
  status: TimingStageStatus;
  startPercent: number;
  widthPercent: number;
  durationMs: number;
}

export function resolveRequestedEffective(
  field: RequestedEffective<string> | null | undefined,
): string | null {
  return field?.effective ?? field?.requested ?? null;
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
  const axis = parseNs(detail.timeline_end_at_ns) ?? 0n;
  if (axis <= 0n) return [];

  const timing = detail.summary.timing;
  const requestStarted = parseNs(timing.upstream_request_started_at_ns);
  const requestCompleted = parseNs(timing.upstream_request_body_completed_at_ns);
  const responseHeaders = parseNs(timing.upstream_response_headers_at_ns);
  const responseCompleted = parseNs(timing.upstream_response_body_completed_at_ns);
  const finished = parseNs(timing.finished_at_ns);
  const protocol = detail.summary.protocol;
  const firstToken = parseNs(protocol?.first_token_at_ns);
  const active = detail.state === "active";
  const responseMode = protocol?.response_mode.observed ?? protocol?.response_mode.requested;
  const streaming = responseMode === "stream";
  const stages: TimingStage[] = [];
  let cursor = 0n;

  function addStage(label: string, tone: TimingStageTone, boundary: bigint | null): boolean {
    if (boundary !== null) {
      if (boundary < cursor || boundary > axis) return false;
      stages.push({
        label,
        tone,
        status: "complete",
        startPercent: percent(cursor, axis),
        widthPercent: percent(boundary - cursor, axis),
        durationMs: nsToMs(boundary - cursor),
      });
      cursor = boundary;
      return true;
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
    return false;
  }

  if (!addStage("Proxy setup", "request", requestStarted)) return stages;
  if (!addStage("Request upload", "request", requestCompleted)) return stages;
  if (!addStage("Response wait", "wait", responseHeaders)) return stages;

  if (streaming && firstToken !== null) {
    if (!addStage("First-token wait", "wait", firstToken)) return stages;
    if (!addStage("Response stream", "model", responseCompleted)) return stages;
  } else if (streaming && active && responseCompleted === null) {
    addStage("First-token wait", "wait", null);
    return stages;
  } else if (!addStage("Response body", "model", responseCompleted)) {
    return stages;
  }

  addStage("Finalization", "finalize", finished);
  return stages;
}

const TOKEN_FORMAT = new Intl.NumberFormat("en-US");

export function tokenCount(value: number): string {
  return TOKEN_FORMAT.format(value);
}
