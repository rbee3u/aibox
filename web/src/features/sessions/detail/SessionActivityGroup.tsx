import styles from "@/features/sessions/detail/SessionActivityGroup.module.css";
import conversationStyles from "@/features/sessions/detail/conversation.module.css";
import type { SessionActivityItem } from "@/features/sessions/viewTypes";
import { AlertTriangle, Wrench } from "lucide-react";
import type { SessionApi } from "@/api/sessions";
import { SessionEvidenceDisclosure } from "@/features/sessions/detail/SessionEvidenceDisclosure";
import { activitySummary, isRoutineEvidence } from "@/features/sessions/detail/sessionDetail";
import {
  compactMessageTimestamp,
  toolActivityHeadline,
} from "@/features/sessions/detail/sessionFormat";
import type { SourcedSession } from "@/features/sessions/sessionSource";

import { iconSize } from "@/shared/icons/iconSizes";

interface SessionActivityGroupProps {
  api: SessionApi;
  entries: SessionActivityItem[];
  session: SourcedSession;
  snapshot?: string;
  onTranscriptStale: () => Promise<string | null>;
}

/**
 * Uncontrolled disclosures and stable entry keys preserve open groups and tool
 * rows across refreshes and arriving results.
 */
export function SessionActivityGroup({
  api,
  entries,
  session,
  snapshot,
  onTranscriptStale,
}: SessionActivityGroupProps) {
  const summary = activitySummary(entries);
  const routine = entries.filter(isRoutineEvidence);
  const shown = entries.filter((entry) => !isRoutineEvidence(entry));

  const renderEntry = (entry: SessionActivityItem) => {
    if (entry.kind === "tool") {
      const headline = toolActivityHeadline(entry.value.summary);
      return (
        <SessionEvidenceDisclosure
          key={`tool:${entry.value.entry_ids[0]}`}
          api={api}
          entryId={entry.value.entry_ids[0]}
          label={
            <>
              <Wrench size={iconSize.xs} aria-hidden="true" /> {entry.value.name}
              {headline ? ` · ${headline}` : ""}
            </>
          }
          meta={compactMessageTimestamp(entry.value.timestamp, session.start_ts)}
          preview={entry.value.summary}
          result={
            entry.result
              ? { entryId: entry.result.entry_ids[0], preview: entry.result.summary }
              : undefined
          }
          session={session}
          snapshot={snapshot}
          status="tool"
          toolStatus={entry.value.status}
          onTranscriptStale={onTranscriptStale}
        />
      );
    }
    return (
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
        onTranscriptStale={onTranscriptStale}
      />
    );
  };

  return (
    <details className={conversationStyles.sessionActivityGroup}>
      <summary>
        <span>
          {summary.toolCount > 0 ? <Wrench size={iconSize.xs} aria-hidden="true" /> : null}
          {summary.title}
          {summary.hasIssue && (
            <AlertTriangle size={iconSize.xs} aria-label="Activity has diagnostics" />
          )}
        </span>
        {summary.detail && (
          <span className={conversationStyles.sessionRowMeta}>{summary.detail}</span>
        )}
      </summary>
      <div className={conversationStyles.sessionActivityGroupItems}>
        {shown.map(renderEntry)}
        {routine.length > 0 && (
          <details className={styles.sessionRoutineEntries}>
            <summary>
              {routine.length} routine {routine.length === 1 ? "entry" : "entries"}
            </summary>
            <div className={conversationStyles.sessionActivityGroupItems}>
              {routine.map(renderEntry)}
            </div>
          </details>
        )}
      </div>
    </details>
  );
}
