import { sessionListCopy } from "@/features/sessions/sessionListCopy";
import { visibleSessionListSource, type SourcedSession } from "@/features/sessions/sessionSource";
import { formatTimestamp } from "@/shared/lib/format";
import type { ConfirmDialogFact } from "@/shared/ui/ConfirmDialog";

/** Catalog-row facts for a single-session delete confirmation. */
export function sessionDeletionFacts(session: SourcedSession): ConfirmDialogFact[] {
  return [
    { label: "Session", value: sessionListCopy(session.title, session.latest_message).headline },
    { label: "Source", value: visibleSessionListSource(session.source) },
    { label: "Started", value: formatTimestamp(session.start_ts) },
  ];
}
