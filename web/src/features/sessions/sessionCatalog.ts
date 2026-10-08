import type { SessionListData } from "@/api/sessions";
import {
  compareSessions,
  sourcedSession,
  type AggregatedSessionData,
  type SessionSource,
} from "@/features/sessions/sessionSource";

/** A completed catalog read, delivered before its request lease is released. */
export type SessionCatalogResult =
  { kind: "loaded"; data: AggregatedSessionData } | { kind: "failed"; cause: unknown };

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

export function messageCountLabel(count: number): string {
  return `${count} message${count === 1 ? "" : "s"}`;
}

export function toolCountLabel(count: number): string {
  return `${count} tool${count === 1 ? "" : "s"}`;
}
