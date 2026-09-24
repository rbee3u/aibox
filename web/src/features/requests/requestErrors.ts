import { messageOrFallback, wasCancelled } from "@/shared/lib/errors";

/** Wording the Requests module shows when a rejection carries no message. */
export const REQUEST_FAILURE_FALLBACK = "Requests API call failed";

export function requestErrorMessage(cause: unknown): string {
  return messageOrFallback(cause, REQUEST_FAILURE_FALLBACK);
}

export const requestWasCancelled = wasCancelled;
