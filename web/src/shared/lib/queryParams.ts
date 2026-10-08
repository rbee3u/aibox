export function searchString(params: URLSearchParams): string {
  const query = params.toString();
  return query ? `?${query}` : "";
}

export function readPositiveInteger(
  params: URLSearchParams,
  key: string,
  fallback: number,
): number {
  const raw = params.get(key);
  const parsed = raw && /^\d+$/.test(raw) ? Number(raw) : fallback;
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

export function readEnum<T extends string>(
  params: URLSearchParams,
  key: string,
  allowed: readonly T[],
  fallback: T,
): T {
  const raw = params.get(key);
  return allowed.includes(raw as T) ? (raw as T) : fallback;
}

export function readTrimmed(params: URLSearchParams, key: string): string | null {
  return params.get(key)?.trim() || null;
}
