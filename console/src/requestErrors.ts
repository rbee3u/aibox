export function requestErrorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Requests API call failed";
}

export function requestWasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
}
