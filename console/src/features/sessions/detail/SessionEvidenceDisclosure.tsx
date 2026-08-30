import { Clipboard } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";
import type { SessionApi, TranscriptEvidence } from "@/api/sessions";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import { messageOf } from "@/shared/lib/errors";
import styles from "@/features/sessions/SessionPage.module.css";

interface SessionEvidenceDisclosureProps {
  api: SessionApi;
  entryId: string;
  label: ReactNode;
  meta: string;
  preview: string;
  session: SourcedSession;
  snapshot?: string;
  status: string;
}

/**
 * One Transcript record inside an activity group. Raw evidence loads only when
 * the reader opens the disclosure, and reasoning records stay hidden.
 */
export function SessionEvidenceDisclosure({
  api,
  entryId,
  label,
  meta,
  preview,
  session,
  snapshot,
  status,
}: SessionEvidenceDisclosureProps) {
  const [evidence, setEvidence] = useState<TranscriptEvidence | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hidden = status === "hidden_internal";

  async function loadEvidence() {
    if (evidence || loading || hidden || !snapshot) return;
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
      className={status === "tool" ? styles.sessionActivity : styles.sessionEvidence}
      onToggle={(event) => {
        if (event.currentTarget.open) void loadEvidence();
      }}
    >
      <summary>
        <span>{label}</span>
        <span>{meta}</span>
      </summary>
      {preview && <pre>{preview}</pre>}
      {hidden && <p>Internal reasoning is intentionally hidden.</p>}
      {!hidden && !snapshot && (
        <p>Full evidence is available after the Transcript finishes loading.</p>
      )}
      {loading && <p>Loading Transcript Entry…</p>}
      {error && <p className={styles.sessionEvidenceError}>{error}</p>}
      {evidence && (
        <div className={styles.sessionEvidenceRaw}>
          <button
            type="button"
            onClick={() => void navigator.clipboard.writeText(evidence.content)}
          >
            <Clipboard size={13} aria-hidden="true" /> Copy {evidence.encoding}
          </button>
          <pre>{evidence.content}</pre>
        </div>
      )}
    </details>
  );
}
