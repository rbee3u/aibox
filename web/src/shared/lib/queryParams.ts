/**
 * Primitives shared by the per-module query codecs. Each module owns a
 * `route.ts` that reads a `search` snapshot into a typed selection and writes it
 * back, so invalid values always collapse to the canonical default URL.
 */

/** Renders a query string with the leading `?`, or an empty string when blank. */
export function searchString(params: URLSearchParams): string {
  const query = params.toString();
  return query ? `?${query}` : "";
}

/** Reads a positive integer, falling back for missing, malformed, or unsafe values. */
export function readPositiveInteger(
  params: URLSearchParams,
  key: string,
  fallback: number,
): number {
  const raw = params.get(key);
  const parsed = raw && /^\d+$/.test(raw) ? Number(raw) : fallback;
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

/** Reads a value constrained to `allowed`, falling back otherwise. */
export function readEnum<T extends string>(
  params: URLSearchParams,
  key: string,
  allowed: readonly T[],
  fallback: T,
): T {
  const raw = params.get(key);
  return allowed.includes(raw as T) ? (raw as T) : fallback;
}

/** Reads a trimmed value, treating blank as absent. */
export function readTrimmed(params: URLSearchParams, key: string): string | null {
  return params.get(key)?.trim() || null;
}
