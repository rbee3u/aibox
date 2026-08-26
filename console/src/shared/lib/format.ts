const UTC_PLUS_EIGHT_MS = 8 * 60 * 60 * 1000;
const EXPLICIT_TIME_ZONE = /(?:z|[+-]\d{2}:\d{2})$/i;

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

/**
 * Decimal-prefix byte sizes (KB/MB). Binary-prefix sizes come from
 * `formatBinaryByteSize` in `api/encoding`; both spellings exist so each
 * surface keeps its established wording.
 */
export function formatByteSize(value: number | null | undefined): string {
  if (value == null) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1048576) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1048576).toFixed(1)} MB`;
}

export function hex(bytesValue: Uint8Array): string {
  return Array.from(bytesValue, (value) => value.toString(16).padStart(2, "0")).join(" ");
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
