import type { RequestList, RequestSummary } from "@/api/requests";

export const REQUESTS_PER_PAGE = 50;

/**
 * Chooses which row should receive focus after `deletedId` disappears. Focus
 * follows the nearest remaining deletable neighbour, preferring the row after
 * the deleted one, and moves to the last row when the list fell back a page.
 */
export function focusTargetAfterDelete(
  before: RequestSummary[],
  deletedId: string,
  after: RequestSummary[],
  movedToPreviousPage: boolean,
): string | null {
  const deletableAfter = after.filter((request) => request.state !== "active");
  if (deletableAfter.length === 0) return null;
  if (movedToPreviousPage) return deletableAfter.at(-1)?.id ?? null;

  const deletedIndex = before.findIndex((request) => request.id === deletedId);
  if (deletedIndex >= 0) {
    const adjacentIds = [
      ...before.slice(deletedIndex + 1),
      ...before.slice(0, deletedIndex).reverse(),
    ]
      .filter((request) => request.state !== "active")
      .map((request) => request.id);
    const remainingIds = new Set(deletableAfter.map((request) => request.id));
    const adjacentId = adjacentIds.find((id) => remainingIds.has(id));
    if (adjacentId) return adjacentId;
  }

  const start = Math.min(Math.max(deletedIndex, 0), after.length - 1);
  return (
    after.slice(start).find((request) => request.state !== "active")?.id ??
    after
      .slice(0, start)
      .reverse()
      .find((request) => request.state !== "active")?.id ??
    null
  );
}

/** Applies a completed deletion to the page the Console is already showing. */
export function removeDeletedFromList(
  current: RequestList,
  ids: readonly string[],
  deletedCount: number,
  currentPage: number,
): RequestList {
  const deleted = new Set(ids);
  const total = Math.max(0, current.total - deletedCount);
  return {
    ...current,
    requests: current.requests.filter((request) => !deleted.has(request.id)),
    total,
    deletable_count: Math.max(0, current.deletable_count - deletedCount),
    has_next: currentPage * REQUESTS_PER_PAGE < total,
  };
}
