import { tryDecodeBase64 } from "@/shared/lib/encoding";
import type { EventTimingIndex, HeaderValue, RequestSummary } from "@/api/requests";
import { hex } from "@/shared/lib/format";

type RequestTarget = Pick<RequestSummary, "upstream_url" | "incoming_uri">;

function parseUpstreamUrl(request: RequestTarget): URL | null {
  try {
    return new URL(request.upstream_url ?? "");
  } catch {
    return null;
  }
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

export function decodeHeader(header: HeaderValue): string {
  const decoded = tryDecodeHeader(header);
  if (decoded !== null) return decoded;
  const bytesValue = tryDecodeBase64(header.value_base64);
  if (bytesValue === null) {
    return "[invalid base64 header value]";
  }
  return `[hex] ${hex(bytesValue)}`;
}

export function tryDecodeHeader(header: HeaderValue): string | null {
  const bytesValue = tryDecodeBase64(header.value_base64);
  if (bytesValue === null) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytesValue);
  } catch {
    return null;
  }
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
