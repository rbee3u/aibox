import type { HeaderValue, RecordSummary } from "./types";

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

export function recordUrl(
  record: Pick<RecordSummary, "upstream_url" | "incoming_uri">,
): [string, string] {
  try {
    const url = new URL(record.upstream_url ?? "");
    return [url.host, `${url.pathname}${url.search}`];
  } catch {
    return ["invalid target", record.incoming_uri];
  }
}

export function hex(bytesValue: Uint8Array): string {
  return Array.from(bytesValue, (value) => value.toString(16).padStart(2, "0")).join(" ");
}

export function decodeBytes(bytesValue: Uint8Array, label: string): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytesValue);
  } catch {
    return `[non-UTF-8 ${label}; hex view]\n${hex(bytesValue)}`;
  }
}

export function decodeHeader(header: HeaderValue): string {
  let binary: string;
  try {
    binary = window.atob(header.value_base64);
  } catch {
    return "[invalid base64 header value]";
  }
  const bytesValue = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytesValue);
  } catch {
    return `[hex] ${hex(bytesValue)}`;
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

export function queryParams(urlValue: string | null): Array<[string, string]> {
  if (!urlValue) return [];
  try {
    return [...new URL(urlValue).searchParams.entries()];
  } catch {
    return [];
  }
}
