import type { EventTimingIndex, HeaderValue, RequestSummary } from "./types";

const UTC_PLUS_EIGHT_MS = 8 * 60 * 60 * 1000;
const EXPLICIT_TIME_ZONE = /(?:z|[+-]\d{2}:\d{2})$/i;
type RequestTarget = Pick<RequestSummary, "upstream_url" | "incoming_uri">;

function twoDigits(value: number): string {
  return value.toString().padStart(2, "0");
}

export function formatTimestamp(value: string): string {
  return formatUtcPlusEight(value);
}

export function formatTimestampWithMilliseconds(value: string): string {
  return formatUtcPlusEight(value, true);
}

function formatUtcPlusEight(value: string, includeMilliseconds = false): string {
  if (!EXPLICIT_TIME_ZONE.test(value)) return "—";
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "—";

  const eastEight = new Date(timestamp + UTC_PLUS_EIGHT_MS);
  if (!Number.isFinite(eastEight.getTime())) return "—";
  const date = `${eastEight.getUTCFullYear()}-${twoDigits(eastEight.getUTCMonth() + 1)}-${twoDigits(eastEight.getUTCDate())}`;
  const time = `${twoDigits(eastEight.getUTCHours())}:${twoDigits(eastEight.getUTCMinutes())}:${twoDigits(eastEight.getUTCSeconds())}`;
  const milliseconds = includeMilliseconds
    ? `.${eastEight.getUTCMilliseconds().toString().padStart(3, "0")}`
    : "";
  return `${date} ${time}${milliseconds}`;
}

export function compactDuration(ms: number | null | undefined): string {
  if (ms == null) return "—";
  if (ms < 1000) return `${Math.round(ms)}ms`;

  const totalSeconds = Math.round(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return [
    hours > 0 ? `${hours}h` : "",
    minutes > 0 ? `${minutes}m` : "",
    seconds > 0 ? `${seconds}s` : "",
  ]
    .filter(Boolean)
    .join("");
}

export function duration(ms: number | null | undefined): string {
  if (ms == null) return "—";
  return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`;
}

export function bytes(value: number | null | undefined): string {
  if (value == null) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1048576) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1048576).toFixed(1)} MB`;
}

export function requestUrl(request: RequestTarget): {
  host: string;
  path: string;
  label: string;
  title: string;
} {
  const url = parseUpstreamUrl(request);
  if (url) {
    return {
      host: url.host,
      path: url.pathname,
      label: `${url.host}${url.pathname}`,
      title: request.upstream_url ?? "",
    };
  }

  const host = "invalid target";
  const path = request.incoming_uri;
  return {
    host,
    path,
    label: [host, path].filter(Boolean).join(" "),
    title: request.incoming_uri,
  };
}

export function requestDetailUrl(request: RequestTarget): [string, string] {
  const url = parseUpstreamUrl(request);
  return url
    ? [url.origin, `${url.pathname}${url.search}`]
    : ["invalid target", request.incoming_uri];
}

function parseUpstreamUrl(request: RequestTarget): URL | null {
  try {
    return new URL(request.upstream_url ?? "");
  } catch {
    return null;
  }
}

export function hex(bytesValue: Uint8Array): string {
  return Array.from(bytesValue, (value) => value.toString(16).padStart(2, "0")).join(" ");
}

export function decodeHeader(header: HeaderValue): string {
  const decoded = tryDecodeHeader(header);
  if (decoded !== null) return decoded;
  const bytesValue = decodeBase64(header.value_base64);
  if (bytesValue === null) {
    return "[invalid base64 header value]";
  }
  return `[hex] ${hex(bytesValue)}`;
}

export function tryDecodeHeader(header: HeaderValue): string | null {
  const bytesValue = decodeBase64(header.value_base64);
  if (bytesValue === null) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytesValue);
  } catch {
    return null;
  }
}

function decodeBase64(value: string): Uint8Array | null {
  try {
    const binary = window.atob(value);
    return Uint8Array.from(binary, (character) => character.charCodeAt(0));
  } catch {
    return null;
  }
}

export function concatChunks(chunks: Uint8Array[]): Uint8Array {
  const output = new Uint8Array(chunks.reduce((size, chunk) => size + chunk.length, 0));
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.length;
  }
  return output;
}

export function mergeEventTimings(
  current: EventTimingIndex | null,
  incoming: EventTimingIndex,
): EventTimingIndex {
  const bySequence = new Map(
    (current?.events ?? []).map((event) => [event.sequence, event] as const),
  );
  for (const event of incoming.events) bySequence.set(event.sequence, event);
  return {
    state: incoming.state,
    events: [...bySequence.values()].sort((left, right) => left.sequence - right.sequence),
    next_sequence: Math.max(current?.next_sequence ?? 0, incoming.next_sequence),
    warning: incoming.warning,
  };
}
