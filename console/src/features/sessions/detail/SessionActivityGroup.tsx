import { AlertTriangle, Wrench } from "lucide-react";
import { useEffect, useRef } from "react";
import type { SessionApi } from "@/api/sessions";
import { SessionEvidenceDisclosure } from "@/features/sessions/detail/SessionEvidenceDisclosure";
import {
  activitySummary,
  type SessionActivityItem,
} from "@/features/sessions/detail/sessionDetail";
import { compactMessageTimestamp } from "@/features/sessions/detail/sessionFormat";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import styles from "@/features/sessions/SessionPage.module.css";

interface SessionActivityGroupProps {
  api: SessionApi;
  entries: SessionActivityItem[];
  /** Reloading the Session collapses every activity disclosure again. */
  reloadRevision: number;
  session: SourcedSession;
  snapshot?: string;
}

export function SessionActivityGroup({
  api,
  entries,
  reloadRevision,
  session,
  snapshot,
}: SessionActivityGroupProps) {
  const disclosureRef = useRef<HTMLDetailsElement>(null);
  const summary = activitySummary(entries);
  const activityLabels =
    summary.labels.length > 0
      ? `${summary.labels.slice(0, 3).join(", ")}${summary.labels.length > 3 ? ` +${summary.labels.length - 3}` : ""}`
      : "Transcript events";

  useEffect(() => {
    if (disclosureRef.current) disclosureRef.current.open = false;
  }, [reloadRevision]);

  return (
    <details ref={disclosureRef} className={styles.sessionActivityGroup}>
      <summary>
        <span>
          <Wrench size={13} aria-hidden="true" /> Transcript activity
          {summary.hasIssue && <AlertTriangle size={13} aria-label="Activity has diagnostics" />}
        </span>
        <span>
          {summary.count} {summary.count === 1 ? "item" : "items"} · {activityLabels}
        </span>
      </summary>
      <div className={styles.sessionActivityGroupItems}>
        {entries.map((entry) =>
          entry.kind === "tool" ? (
            <SessionEvidenceDisclosure
              key={`tool:${entry.value.entry_ids.join(",")}`}
              api={api}
              entryId={entry.value.entry_ids[0]}
              label={
                <>
                  <Wrench size={13} aria-hidden="true" /> {entry.value.name}
                </>
              }
              meta={
                ["started", "completed"].includes(entry.value.status)
                  ? compactMessageTimestamp(entry.value.timestamp, session.start_ts)
                  : entry.value.status
              }
              preview={entry.value.summary}
              session={session}
              snapshot={snapshot}
              status="tool"
            />
          ) : (
            <SessionEvidenceDisclosure
              key={entry.value.entry_id}
              api={api}
              entryId={entry.value.entry_id}
              label={entry.value.native_type}
              meta={`${entry.value.status} · ${compactMessageTimestamp(entry.value.timestamp, session.start_ts)}`}
              preview={entry.value.preview}
              session={session}
              snapshot={snapshot}
              status={entry.value.status}
            />
          ),
        )}
      </div>
    </details>
  );
}
