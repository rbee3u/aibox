import type { SessionListData } from "@/api/sessions";
import { messageOf } from "@/shared/lib/errors";
import {
  compareSessions,
  sourcedSession,
  visibleSessionSource,
  type AggregatedSessionData,
  type SessionSource,
  type SourcedSession,
} from "@/features/sessions/sessionSource";

export interface SessionCatalogSuccess {
  source: SessionSource;
  result: SessionListData;
}

export interface SessionCatalogFailure {
  source: SessionSource;
  cause: unknown;
}

/** Separates settled source reads while retaining their stable source identity. */
export function splitSessionResults(
  results: readonly PromiseSettledResult<SessionCatalogSuccess>[],
  sources: readonly SessionSource[],
): { successes: SessionCatalogSuccess[]; failures: SessionCatalogFailure[] } {
  const successes: SessionCatalogSuccess[] = [];
  const failures: SessionCatalogFailure[] = [];
  results.forEach((result, index) => {
    if (result.status === "fulfilled") successes.push(result.value);
    else failures.push({ source: sources[index], cause: result.reason });
  });
  return { successes, failures };
}

/** Projects successful and failed source reads into the list contract. */
export function aggregateSessionCatalog(
  successes: readonly SessionCatalogSuccess[],
  failures: readonly SessionCatalogFailure[],
): AggregatedSessionData {
  const warnings = [
    ...failures.map(({ cause, source }) => `${visibleSessionSource(source)}: ${messageOf(cause)}`),
    ...successes.flatMap(({ result, source }) =>
      result.warnings.map((warning) => `${visibleSessionSource(source)}: ${warning}`),
    ),
  ];
  const sessions = successes
    .flatMap(({ result, source }) => result.sessions.map((row) => sourcedSession(source, row)))
    .sort(compareSessions);
  return {
    sessions,
    warnings,
    partial: failures.length > 0 || successes.some(({ result }) => result.partial),
  };
}

export interface SessionDeletionGroup {
  source: SessionSource;
  ids: string[];
}

/** Groups a batch by Tenant-and-Coding Agent in deterministic request order. */
export function groupSessionsForDeletion(rows: readonly SourcedSession[]): SessionDeletionGroup[] {
  const groups = new Map<string, SessionDeletionGroup>();
  for (const row of rows) {
    const group = groups.get(row.source.key) ?? { source: row.source, ids: [] };
    group.ids.push(row.id);
    groups.set(row.source.key, group);
  }
  return [...groups.values()].sort((left, right) =>
    left.source.key.localeCompare(right.source.key),
  );
}

export interface SessionDialogSource {
  source: SessionSource;
  count: number;
}

export function sessionDialogSources(rows: readonly SourcedSession[]): SessionDialogSource[] {
  return groupSessionsForDeletion(rows).map(({ source, ids }) => ({ source, count: ids.length }));
}
