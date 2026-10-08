import styles from "@/features/sessions/detail/SessionEvidenceDisclosure.module.css";
import conversationStyles from "@/features/sessions/detail/conversation.module.css";
import { Check, Clipboard } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";
import {
  isTranscriptConflict,
  type SessionApi,
  type ToolActivity,
  type TranscriptEvidence,
} from "@/api/sessions";
import { toolNeedsAttention } from "@/features/sessions/detail/sessionDetail";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { messageOf } from "@/shared/lib/errors";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { StatusBadge, type StatusTone } from "@/shared/ui/StatusBadge";

import { iconSize } from "@/shared/icons/iconSizes";

interface SessionEvidenceDisclosureProps {
  api: SessionApi;
  entryId: string;
  label: ReactNode;
  meta: string;
  preview: string;
  /** For a tool: the record that came back, once it has. */
  result?: { entryId: string; preview: string };
  session: SourcedSession;
  snapshot?: string;
  status: string;
  toolStatus?: ToolActivity["status"];
  /** Re-reads the Session after the Transcript changed; resolves to the new snapshot. */
  onTranscriptStale: () => Promise<string | null>;
}

/**
 * No result is neutral because the Transcript cannot establish failure.
 */
const TOOL_STATUS_BADGE: Partial<
  Record<ToolActivity["status"], { label: string; tone: StatusTone }>
> = {
  started: { label: "No result", tone: "neutral" },
  incomplete: { label: "No result", tone: "neutral" },
  failed: { label: "Failed", tone: "error" },
  unknown: { label: "Unknown", tone: "error" },
};

function prettyEvidence(content: string): string {
  try {
    return JSON.stringify(JSON.parse(content), null, 2);
  } catch {
    return content;
  }
}

/**
 * Evidence reads are snapshot-bound. Refresh the Session once on stale
 * evidence, then retry against the new snapshot.
 */
function RawEntry({
  api,
  entryId,
  label,
  session,
  snapshot,
  onTranscriptStale,
}: {
  api: SessionApi;
  entryId: string;
  label: string;
  session: SourcedSession;
  snapshot: string;
  onTranscriptStale: () => Promise<string | null>;
}) {
  const [evidence, setEvidence] = useState<TranscriptEvidence | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [copied, copy] = useClipboardFeedback();

  async function load() {
    if (evidence || loading) return;
    setLoading(true);
    setError(null);
    setStale(false);
    const read = (at: string) =>
      api.loadSessionEvidence(session.source.tenant, session.source.agent, session.id, entryId, at);
    try {
      setEvidence(await read(snapshot));
    } catch (cause) {
      if (!isTranscriptConflict(cause)) {
        setError(messageOf(cause));
      } else {
        try {
          const current = await onTranscriptStale();
          if (current === null) throw cause;
          setEvidence(await read(current));
        } catch (retryCause) {
          if (isTranscriptConflict(retryCause)) setStale(true);
          else setError(messageOf(retryCause));
        }
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <details
      className={conversationStyles.sessionEvidenceRaw}
      onToggle={(event) => {
        if (event.currentTarget.open) void load();
      }}
    >
      <summary>{label}</summary>
      {loading && <p>Loading Transcript Entry…</p>}
      {error && <p className={styles.sessionEvidenceError}>{error}</p>}
      {stale && (
        <p className={styles.sessionEvidenceStale}>
          This Session has grown since it was opened.
          <RefreshButton
            label={`Retry ${label.toLowerCase()}`}
            tone="secondary"
            onClick={() => void load()}
          >
            Retry
          </RefreshButton>
        </p>
      )}
      {evidence && (
        <>
          <button
            type="button"
            aria-label={copied ? "Raw entry copied" : `Copy ${label.toLowerCase()}`}
            onClick={() => void copy(evidence.content, true)}
          >
            {copied ? (
              <Check size={iconSize.xs} aria-hidden="true" />
            ) : (
              <Clipboard size={iconSize.xs} aria-hidden="true" />
            )}{" "}
            {copied ? "Copied" : "Copy"}
          </button>
          <pre>{prettyEvidence(evidence.content)}</pre>
        </>
      )}
    </details>
  );
}

/**
 * Load raw evidence only on disclosure; internal reasoning stays hidden.
 */
export function SessionEvidenceDisclosure({
  api,
  entryId,
  label,
  meta,
  preview,
  result,
  session,
  snapshot,
  status,
  toolStatus,
  onTranscriptStale,
}: SessionEvidenceDisclosureProps) {
  const hidden = status === "hidden_internal";
  const isTool = status === "tool";
  const attention = toolStatus !== undefined && toolNeedsAttention(toolStatus);
  const badge = toolStatus !== undefined ? TOOL_STATUS_BADGE[toolStatus] : undefined;

  return (
    <details
      className={isTool ? conversationStyles.sessionActivity : conversationStyles.sessionEvidence}
    >
      <summary>
        <span>{label}</span>
        {badge && (
          <StatusBadge tone={badge.tone} variant="inline" size="xs">
            {badge.label}
          </StatusBadge>
        )}
        <span className={conversationStyles.sessionRowMeta}>{meta}</span>
      </summary>
      {isTool ? (
        <div className={styles.sessionToolExchange}>
          {preview && (
            <section>
              <h4>Input</h4>
              <pre>{preview}</pre>
            </section>
          )}
          {result?.preview && (
            <section className={attention ? styles.sessionToolResultFailed : undefined}>
              <h4>Result</h4>
              <pre>{result.preview}</pre>
            </section>
          )}
        </div>
      ) : (
        preview && <pre>{preview}</pre>
      )}
      {hidden && <p>Internal reasoning is intentionally hidden.</p>}
      {!hidden && !snapshot && (
        <p>Full evidence is available after the Transcript finishes loading.</p>
      )}
      {!hidden && snapshot && (
        <div className={styles.sessionEvidenceRawList}>
          <RawEntry
            api={api}
            entryId={entryId}
            label={isTool ? "Raw call entry" : "Raw entry"}
            session={session}
            snapshot={snapshot}
            onTranscriptStale={onTranscriptStale}
          />
          {isTool && result && (
            <RawEntry
              api={api}
              entryId={result.entryId}
              label="Raw result entry"
              session={session}
              snapshot={snapshot}
              onTranscriptStale={onTranscriptStale}
            />
          )}
        </div>
      )}
    </details>
  );
}
