import { Check, Clipboard } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";
import type { SessionApi, ToolActivity, TranscriptEvidence } from "@/api/sessions";
import { toolNeedsAttention } from "@/features/sessions/detail/sessionDetail";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { messageOf } from "@/shared/lib/errors";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/sessions/SessionPage.module.css";
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
}

const TOOL_STATUS_LABEL: Record<ToolActivity["status"], string> = {
  started: "Running",
  completed: "Completed",
  failed: "Failed",
  incomplete: "Incomplete",
  unknown: "Unknown",
};

function prettyEvidence(content: string): string {
  try {
    return JSON.stringify(JSON.parse(content), null, 2);
  } catch {
    return content;
  }
}

/** One raw Transcript Entry, loaded on demand and rendered indented. */
function RawEntry({
  api,
  entryId,
  label,
  session,
  snapshot,
}: {
  api: SessionApi;
  entryId: string;
  label: string;
  session: SourcedSession;
  snapshot: string;
}) {
  const [evidence, setEvidence] = useState<TranscriptEvidence | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, copy] = useClipboardFeedback();

  async function load() {
    if (evidence || loading) return;
    setLoading(true);
    setError(null);
    try {
      setEvidence(
        await api.loadSessionEvidence(
          session.source.tenant,
          session.source.agent,
          session.id,
          entryId,
          snapshot,
        ),
      );
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setLoading(false);
    }
  }

  return (
    <details
      className={styles.sessionEvidenceRaw}
      onToggle={(event) => {
        if (event.currentTarget.open) void load();
      }}
    >
      <summary>{label}</summary>
      {loading && <p>Loading Transcript Entry…</p>}
      {error && <p className={styles.sessionEvidenceError}>{error}</p>}
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
 * One Transcript record inside an activity group. A tool row shows what was
 * asked and what came back; raw Transcript Entries load only when opened, and
 * reasoning records stay hidden.
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
}: SessionEvidenceDisclosureProps) {
  const hidden = status === "hidden_internal";
  const isTool = status === "tool";
  const attention = toolStatus !== undefined && toolNeedsAttention(toolStatus);

  return (
    <details className={isTool ? styles.sessionActivity : styles.sessionEvidence}>
      <summary>
        <span>{label}</span>
        {attention && toolStatus && (
          <StatusBadge tone="error" variant="inline" size="xs">
            {TOOL_STATUS_LABEL[toolStatus]}
          </StatusBadge>
        )}
        <span className={styles.sessionRowMeta}>{meta}</span>
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
          {toolStatus === "started" && <p>No result recorded.</p>}
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
          />
          {isTool && result && (
            <RawEntry
              api={api}
              entryId={result.entryId}
              label="Raw result entry"
              session={session}
              snapshot={snapshot}
            />
          )}
        </div>
      )}
    </details>
  );
}
