import {
  isLosslessNumber,
  parse as parseLosslessJson,
  stringify as stringifyLosslessJson,
  type LosslessNumber,
} from "lossless-json";
import type { BodyKind, HeaderValue, RequestDetail } from "@/api/requests";
import { formatTimestampWithMilliseconds, hex } from "@/shared/lib/format";
import { tryDecodeHeader } from "@/features/requests/requestFormat";

export const LARGE_PRETTY_BYTES = 5 * 1024 * 1024;
export const LONG_STRING_CHARACTERS = 200;

export function shouldDeferPretty(decodedBytes: number): boolean {
  return decodedBytes > LARGE_PRETTY_BYTES;
}

export function jsonStringPreview(value: string): string | null {
  let offset = 0;
  for (
    let characters = 0;
    characters < LONG_STRING_CHARACTERS && offset < value.length;
    characters += 1
  ) {
    const codePoint = value.codePointAt(offset);
    offset += codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
  }
  return offset < value.length ? value.slice(0, offset) : null;
}

interface JsonObject {
  [key: string]: JsonValue;
}

export type JsonValue = null | boolean | string | LosslessNumber | JsonValue[] | JsonObject;

type JsonParseResult = { ok: true; value: JsonValue } | { ok: false; message: string };

export type ContentCoding =
  { kind: "identity" } | { kind: "zstd" } | { kind: "unsupported"; message: string };

export interface ParsedSseEvent {
  sequence: number;
  data: string;
  eventType: string;
  explicitEventType: string | null;
}

interface ParsedSseStream {
  events: ParsedSseEvent[];
  hasPartialTail: boolean;
}

type Utf8Result = { ok: true; text: string } | { ok: false; hex: string };

export function decodeUtf8(bytes: Uint8Array, complete = true): Utf8Result {
  try {
    return {
      ok: true,
      text: new TextDecoder("utf-8", { fatal: true }).decode(bytes, { stream: !complete }),
    };
  } catch {
    return {
      ok: false,
      hex: hex(bytes),
    };
  }
}

export function parseJson(text: string): JsonParseResult {
  try {
    return {
      ok: true,
      value: parseLosslessJson(text, null, {
        onDuplicateKey: ({ key }) => {
          throw new SyntaxError(`Duplicate object key ${JSON.stringify(key)}`);
        },
      }) as JsonValue,
    };
  } catch (cause) {
    return {
      ok: false,
      message: cause instanceof Error ? cause.message : "Body is not valid JSON",
    };
  }
}

export function stringifyJson(value: JsonValue, pretty = false): string {
  return stringifyLosslessJson(value, null, pretty ? 2 : undefined) ?? "null";
}

export function isJsonContainer(value: JsonValue): value is JsonValue[] | JsonObject {
  return (
    Array.isArray(value) ||
    (typeof value === "object" && value !== null && !isLosslessNumber(value))
  );
}

export function jsonEntries(value: JsonValue): Array<[string, JsonValue]> {
  if (Array.isArray(value)) return value.map((entry, index) => [String(index), entry]);
  if (isJsonContainer(value)) return Object.entries(value);
  return [];
}

export function jsonValueType(value: JsonValue): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  if (isLosslessNumber(value)) return "number";
  return typeof value;
}

export function contentCoding(headers: HeaderValue[]): ContentCoding {
  const codings: string[] = [];
  for (const header of headers) {
    if (header.name.toLowerCase() !== "content-encoding") continue;
    const value = tryDecodeHeader(header);
    if (value === null) {
      return { kind: "unsupported", message: "Content-Encoding header is not valid UTF-8" };
    }
    codings.push(
      ...value
        .split(",")
        .map((coding) => coding.trim().toLowerCase())
        .filter(Boolean),
    );
  }
  if (codings.length === 0 || (codings.length === 1 && codings[0] === "identity")) {
    return { kind: "identity" };
  }
  if (codings.length === 1 && codings[0] === "zstd") return { kind: "zstd" };
  return {
    kind: "unsupported",
    message: `Unsupported Content-Encoding: ${codings.join(", ") || "invalid value"}`,
  };
}

export function bodyComplete(detail: RequestDetail, kind: BodyKind): boolean {
  if (detail.state !== "active") return true;
  return kind === "request"
    ? detail.summary.timing.upstream_request_body_completed_at_ns !== null
    : detail.summary.timing.upstream_response_body_completed_at_ns !== null;
}

export function bodyHeaders(detail: RequestDetail, kind: BodyKind): HeaderValue[] {
  return kind === "request" ? detail.request.headers : (detail.response?.headers ?? []);
}

export function bodyMediaType(headers: HeaderValue[]): string | null {
  const header = headers.find((candidate) => candidate.name.toLowerCase() === "content-type");
  const value = header ? tryDecodeHeader(header) : null;
  return value?.split(";", 1)[0].trim().toLowerCase() || null;
}

export function isJsonMediaType(mediaType: string | null): boolean {
  if (!mediaType) return false;
  const slash = mediaType.indexOf("/");
  if (slash < 0) return false;
  const subtype = mediaType.slice(slash + 1);
  return subtype === "json" || subtype.endsWith("+json");
}

export function isSseResponse(detail: RequestDetail): boolean {
  if (!detail.response) return false;
  const mediaType = bodyMediaType(detail.response.headers);
  return (
    mediaType === "text/event-stream" ||
    detail.summary.protocol?.response_mode.observed === "stream"
  );
}

export function parseSse(text: string): ParsedSseStream {
  const source = text.startsWith("\uFEFF") ? text.slice(1) : text;
  const lines = source.split(/\r\n|\r|\n/);
  if (/(?:\r\n|\r|\n)$/.test(source)) lines.pop();

  const events: ParsedSseEvent[] = [];
  let data: string[] = [];
  let eventType = "";
  let blockTouched = false;
  for (const line of lines) {
    if (line === "") {
      if (data.length > 0) {
        events.push({
          sequence: events.length,
          data: data.join("\n"),
          eventType: eventType || "message",
          explicitEventType: eventType || null,
        });
      }
      data = [];
      eventType = "";
      blockTouched = false;
      continue;
    }
    blockTouched = true;
    if (line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") eventType = value;
    else if (field === "data") data.push(value);
  }
  return { events, hasPartialTail: blockTouched };
}

export function sseEventTypes(
  event: ParsedSseEvent,
  parsed = parseJson(event.data),
): {
  primary: string;
  secondary: string | null;
} {
  const payload =
    parsed.ok && isJsonContainer(parsed.value) && !Array.isArray(parsed.value)
      ? parsed.value
      : undefined;
  const payloadType = payload?.type;
  const payloadObject = payload?.object;
  const primary =
    typeof payloadType === "string" && payloadType
      ? payloadType
      : typeof payloadObject === "string" && payloadObject
        ? payloadObject
        : event.eventType;
  const secondary =
    event.explicitEventType &&
    event.explicitEventType !== "message" &&
    event.explicitEventType !== primary
      ? event.explicitEventType
      : null;
  return { primary, secondary };
}

export function eventRelativeTime(offsetNs: string): string {
  let ns: bigint;
  try {
    ns = BigInt(offsetNs);
  } catch {
    return "Time unavailable";
  }
  const milliseconds = Number(ns) / 1_000_000;
  if (!Number.isFinite(milliseconds)) return "Time unavailable";
  if (milliseconds < 1000) return `+${trimZeros(milliseconds.toFixed(3))} ms`;
  const roundedMilliseconds = Number((ns + 500_000n) / 1_000_000n);
  return `+${trimZeros((roundedMilliseconds / 1000).toFixed(3))} s`;
}

export function eventAbsoluteTime(observedAt: string, offsetNs: string): string {
  const observed = Date.parse(observedAt);
  if (!Number.isFinite(observed)) return "—";
  let milliseconds: number;
  try {
    milliseconds = Number((BigInt(offsetNs) + 500_000n) / 1_000_000n);
  } catch {
    return "—";
  }
  const timestamp = observed + milliseconds;
  if (!Number.isFinite(milliseconds) || !Number.isFinite(timestamp)) return "—";
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "—";
  return formatTimestampWithMilliseconds(date.toISOString());
}

function trimZeros(value: string): string {
  return value.includes(".") ? value.replace(/0+$/, "").replace(/\.$/, "") : value;
}
