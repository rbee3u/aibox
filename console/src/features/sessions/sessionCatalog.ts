import type { SessionListData } from "@/api/sessions";
import {
  compareSessions,
  sourcedSession,
  type AggregatedSessionData,
  type SessionSource,
} from "@/features/sessions/sessionSource";

/** Projects a single source read into the list contract. */
export function projectSessionCatalog(
  source: SessionSource,
  result: SessionListData,
): AggregatedSessionData {
  const sessions = result.sessions.map((row) => sourcedSession(source, row)).sort(compareSessions);
  return {
    sessions,
    warnings: result.warnings,
    partial: result.partial,
  };
}

/** Counts a row and its detail both state, so they must say them the same way. */
export function messageCountLabel(count: number): string {
  return `${count} message${count === 1 ? "" : "s"}`;
}

export function toolCountLabel(count: number): string {
  return `${count} tool${count === 1 ? "" : "s"}`;
}
