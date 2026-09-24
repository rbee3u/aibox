/** Renders an unknown rejection value using its own text. */
export function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** Renders an unknown rejection value, substituting `fallback` for non-Errors. */
export function messageOrFallback(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback;
}

/**
 * Reports whether a rejection came from cancelling `signal`. A caller that
 * aborted its own request sees `signal.aborted`; a rejection raised elsewhere in
 * the same turn is recognized by its `AbortError` name.
 */
export function wasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
}
