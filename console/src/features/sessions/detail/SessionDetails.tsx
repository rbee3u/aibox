import type { SessionDetailMeta, SessionDetailStats } from "@/api/sessions";
import { SessionCopyValue } from "@/features/sessions/detail/SessionCopyValue";
import { sessionListTenantLabel, type SourcedSession } from "@/features/sessions/sessionSource";
import { compactDuration, formatByteSize, formatTimestamp } from "@/shared/lib/format";
import styles from "@/features/sessions/SessionPage.module.css";

interface SessionDetailsProps {
  session: SourcedSession;
  meta: SessionDetailMeta | null;
  stats: SessionDetailStats | null;
  warnings: string[];
  loading: boolean;
  hasDiagnostics: boolean;
  /** The stream ended before reporting completion, so content may be partial. */
  partial: boolean;
}

/** The Details tab: Session facts first, then Transcript diagnostics. */
export function SessionDetails({
  session,
  meta,
  stats,
  warnings,
  loading,
  hasDiagnostics,
  partial,
}: SessionDetailsProps) {
  return (
    <div className={styles.sessionDetailsScroll}>
      <div className={styles.sessionDetailsContent}>
        <section className={styles.sessionDetailsSection}>
          <h3>Session</h3>
          <dl className={styles.sessionDetailsGrid}>
            <div>
              <dt>Tenant</dt>
              <dd>{sessionListTenantLabel(session.source.tenantSelectionValue)}</dd>
            </div>
            <div>
              <dt>Coding Agent</dt>
              <dd>{session.source.agentLabel}</dd>
            </div>
            <div>
              <dt>Session ID</dt>
              <dd>
                <SessionCopyValue label="Session ID" value={meta?.id ?? session.id} />
              </dd>
            </div>
            {meta?.transcript_path && (
              <div>
                <dt>Transcript</dt>
                <dd>
                  <SessionCopyValue label="Transcript path" value={meta.transcript_path} />
                </dd>
              </div>
            )}
            {meta?.cwd && (
              <div>
                <dt>Working directory</dt>
                <dd>
                  <SessionCopyValue label="Working directory" value={meta.cwd} />
                </dd>
              </div>
            )}
            <div>
              <dt>Started</dt>
              <dd>
                <time dateTime={stats?.start_ts ?? session.start_ts}>
                  {formatTimestamp(stats?.start_ts ?? session.start_ts)}
                </time>
              </dd>
            </div>
            {stats?.last_event_ts && (
              <div>
                <dt>Last event</dt>
                <dd>
                  <time dateTime={stats.last_event_ts}>{formatTimestamp(stats.last_event_ts)}</time>
                </dd>
              </div>
            )}
            {stats && (
              <div>
                <dt>Duration</dt>
                <dd>{compactDuration(stats.observed_duration_ms)}</dd>
              </div>
            )}
            {stats && (
              <div>
                <dt>Transcript size</dt>
                <dd>{formatByteSize(stats.file_size)}</dd>
              </div>
            )}
            {meta?.model_provider && (
              <div>
                <dt>Model provider</dt>
                <dd>{meta.model_provider}</dd>
              </div>
            )}
            {meta?.cli_version && (
              <div>
                <dt>CLI version</dt>
                <dd>{meta.cli_version}</dd>
              </div>
            )}
          </dl>
        </section>
        <section className={styles.sessionDetailsSection}>
          <div className={styles.sessionDetailsSectionHeading}>
            <h3>Diagnostics</h3>
            {loading ? (
              <span>Reading Transcript…</span>
            ) : (
              !hasDiagnostics && <span>No transcript diagnostics.</span>
            )}
          </div>
          {stats && hasDiagnostics && (
            <dl className={styles.sessionDiagnosticsGrid}>
              <div>
                <dt>Transcript entries</dt>
                <dd>{stats.entry_count}</dd>
              </div>
              {stats.malformed_count > 0 && (
                <div>
                  <dt>Malformed</dt>
                  <dd>{stats.malformed_count}</dd>
                </div>
              )}
              {stats.unsupported_count > 0 && (
                <div>
                  <dt>Unsupported</dt>
                  <dd>{stats.unsupported_count}</dd>
                </div>
              )}
              {stats.hidden_internal_count > 0 && (
                <div>
                  <dt>Hidden internal</dt>
                  <dd>{stats.hidden_internal_count}</dd>
                </div>
              )}
            </dl>
          )}
          {warnings.length > 0 && (
            <div className={styles.sessionDiagnosticWarnings}>
              {warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          )}
          {partial && (
            <div className={styles.sessionDiagnosticWarnings}>
              <p>Transcript detail did not finish loading. Displayed content may be incomplete.</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
