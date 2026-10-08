import styles from "@/features/requests/detail/RequestSummary.module.css";
import summarySharedStyles from "@/features/requests/detail/summaryShared.module.css";
import { Check, Clipboard } from "lucide-react";

import type { CSSProperties } from "react";
import type { RequestDetail as RequestDetailData } from "@/api/requests";

import { elapsedNsMs, resolveRequestedEffective, timingStages } from "@/features/requests/summary";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { capitalize, duration } from "@/shared/lib/format";

import { IconButton } from "@/shared/ui/IconButton";

import { iconSize } from "@/shared/icons/iconSizes";

import { TokenUsageGroup } from "@/features/requests/detail/TokenUsageGroup";
import { DiagnosticGroup } from "@/features/requests/detail/DiagnosticGroup";
export function RequestSummary({ detail }: { detail: RequestDetailData }) {
  const [copiedSessionId, copySessionIdText] = useClipboardFeedback<string>();
  const total = detail.result?.total_ms ?? detail.live_total_ms;
  const axisMs = elapsedNsMs(detail.timeline_end_at_ns) ?? total ?? 0;
  const protocol = detail.summary.protocol;
  const model =
    resolveRequestedEffective(protocol?.model) ??
    (detail.state === "active" ? "Detecting…" : "Not reported");
  const reasoningEffort = resolveRequestedEffective(protocol?.reasoning_effort);
  const sessionId = detail.summary.agent_session_id;
  const sessionCopied = sessionId !== null && copiedSessionId === sessionId;
  const stages = timingStages(detail);
  const firstToken = elapsedNsMs(protocol?.first_token_at_ns);
  const mode = protocol?.response_mode.observed ?? protocol?.response_mode.requested;
  const responseMode = mode === "stream" ? "Stream" : mode === "normal" ? "Non-stream" : null;
  const diagnostics = detail.diagnostics;
  const hasDiagnostics = Object.values(diagnostics).some((entries) => entries.length > 0);

  function copySessionId() {
    if (sessionId) void copySessionIdText(sessionId, sessionId);
  }

  return (
    <div className={summarySharedStyles.summary}>
      <section className={summarySharedStyles.modelSummary} aria-labelledby="request-model-title">
        <h2 id="request-model-title">Model</h2>
        <div className={styles.modelHeadline}>
          <p className={styles.modelName} title={`Model ${model}`}>
            <span className={styles.modelValue}>{model}</span>
            {reasoningEffort && (
              <>
                {" "}
                <span className={summarySharedStyles.modelEffort}>{reasoningEffort}</span>
              </>
            )}
          </p>
          {responseMode && <span className={styles.modeBadge}>{responseMode}</span>}
        </div>
        <dl className={styles.sessionMeta}>
          <div className={styles.sessionFact}>
            <dt>Agent Session ID</dt>
            <dd>
              <span className={styles.sessionValue}>{sessionId ?? "Not reported"}</span>
              {sessionId && (
                <IconButton
                  size="sm"
                  label={sessionCopied ? "Agent Session ID copied" : "Copy Agent Session ID"}
                  onClick={copySessionId}
                >
                  {sessionCopied ? (
                    <Check size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <Clipboard size={iconSize.xs} aria-hidden="true" />
                  )}
                </IconButton>
              )}
            </dd>
          </div>
        </dl>
      </section>
      <TokenUsageGroup detail={detail} />
      <section className={summarySharedStyles.timingSection} aria-labelledby="request-timing-title">
        <h2 id="request-timing-title">Timing</h2>
        <dl className={styles.timingMetrics}>
          <Metric label="First token" value={duration(firstToken)} />
          <Metric label="Duration" value={duration(total)} />
        </dl>
        {stages.length > 0 ? (
          <div className={styles.timelineContainer}>
            {axisMs > 0 && (
              <div className={styles.timelineRulerRow} aria-hidden="true">
                <span />
                <div className={styles.timelineRuler}>
                  <span className={`${styles.rulerTick} ${styles.rulerTickStart}`}>0 ms</span>
                  <span className={`${styles.rulerTick} ${styles.rulerTickHalf}`}>
                    {duration(axisMs * 0.5)}
                  </span>
                  <span className={`${styles.rulerTick} ${styles.rulerTickEnd}`}>
                    {duration(axisMs)}
                  </span>
                </div>
                <span />
              </div>
            )}
            <div className={styles.timeline} role="list" aria-label="Timing stages">
              {stages.map((stage) => {
                const status = stage.status === "complete" ? "" : ` · ${stage.status}`;
                const value = `${duration(stage.durationMs)}${status}`;
                const startMs = (axisMs * stage.startPercent) / 100;
                const pct = stage.widthPercent.toFixed(1);
                const detailTitle = `${stage.label}: ${value} (${pct}%) · Started at +${duration(startMs)}`;
                const style = {
                  "--stage-start": `${stage.startPercent}%`,
                  "--stage-width": `${stage.widthPercent}%`,
                } as CSSProperties;
                return (
                  <div
                    key={stage.label}
                    className={styles.timelineRow}
                    role="listitem"
                    title={detailTitle}
                    aria-label={detailTitle}
                  >
                    <span className={styles.timelineLabel}>
                      <span
                        className={`${styles.stageDot} ${styles[`tone${capitalize(stage.tone)}`]}`}
                        aria-hidden="true"
                      />
                      {stage.label}
                    </span>
                    <span className={styles.timelineTrack} aria-hidden="true">
                      <span
                        className={`${styles.timelineBar} ${styles[`tone${capitalize(stage.tone)}`]} ${
                          stage.status !== "complete" ? styles.timelinePartial : ""
                        }`}
                        style={style}
                      />
                    </span>
                    <span className={styles.timelineValue}>{value}</span>
                  </div>
                );
              })}
            </div>
            <div className={styles.timelineLegend} aria-label="Timing stage legend">
              <span className={styles.legendItem}>
                <span className={`${styles.stageDot} ${styles.toneRequest}`} aria-hidden="true" />
                <span>Request</span>
              </span>
              <span className={styles.legendItem}>
                <span className={`${styles.stageDot} ${styles.toneWait}`} aria-hidden="true" />
                <span>Waiting (TTFB)</span>
              </span>
              <span className={styles.legendItem}>
                <span className={`${styles.stageDot} ${styles.toneModel}`} aria-hidden="true" />
                <span>Response stream / body</span>
              </span>
              <span className={styles.legendItem}>
                <span className={`${styles.stageDot} ${styles.toneFinalize}`} aria-hidden="true" />
                <span>Finalization</span>
              </span>
            </div>
          </div>
        ) : (
          <p className={styles.sectionState}>Timing stages are not available yet.</p>
        )}
      </section>
      {hasDiagnostics && (
        <section className={styles.diagnostics} aria-labelledby="request-diagnostics-title">
          <h2 id="request-diagnostics-title">Diagnostics</h2>
          <div className={styles.diagnosticGroups}>
            <DiagnosticGroup title="Proxy / transport" entries={diagnostics.request} tone="error" />
            <DiagnosticGroup title="HTTP response" entries={diagnostics.http} tone="error" />
            <DiagnosticGroup title="Model API" entries={diagnostics.provider} tone="error" />
            <DiagnosticGroup title="Warnings" entries={diagnostics.warnings} tone="warning" />
          </div>
        </section>
      )}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.metric}>
      <dt>{label}</dt>
      <dd>
        <span className={styles.metricValue}>{value}</span>
      </dd>
    </div>
  );
}
