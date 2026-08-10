import { Check, Clipboard, FileText } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type {
  AssessmentFinding,
  BodyKind,
  BodyLoadStatus,
  DecodedBodyState,
  DetailTab,
  EventTimingIndex,
  RecordDetail as RecordDetailData,
  TokenUsage,
  UsageState,
} from "../types";
import { createBodyViewMemory, type BodyViewMemory } from "../bodyViewMemory";
import { elapsedNsMs, resolveRequestedEffective, timingStages, tokenCount } from "../summary";
import { decodeHeader, duration, formatTimestamp, recordDetailUrl } from "../utils";
import { BodyViewer } from "./BodyViewer";
import styles from "./RecordDetail.module.css";
import { RecordHeadlineStatus } from "./RecordStatus";
import { assessmentPrimaryLabel } from "./statusPresentation";

const TABS: Array<{ value: DetailTab; label: string }> = [
  { value: "summary", label: "Summary" },
  { value: "request", label: "Request" },
  { value: "response", label: "Response" },
];

interface RecordDetailProps {
  detail: RecordDetailData;
  bodies: Record<BodyKind, Uint8Array[]>;
  bodyStatus: Record<BodyKind, BodyLoadStatus>;
  decodedBodies?: Record<BodyKind, DecodedBodyState>;
  eventTimings?: EventTimingIndex | null;
  tab: DetailTab;
  onTabChange: (tab: DetailTab) => void;
  onDownload: (kind: BodyKind) => void;
  loadingBody: boolean;
}

export function RecordDetail({
  detail,
  bodies,
  bodyStatus,
  decodedBodies = {
    request: { bytes: null, error: null },
    response: { bytes: null, error: null },
  },
  eventTimings = null,
  tab,
  onTabChange,
  onDownload,
  loadingBody,
}: RecordDetailProps) {
  const [bodyViews, setBodyViews] = useState<Record<BodyKind, BodyViewMemory>>({
    request: createBodyViewMemory(),
    response: createBodyViewMemory(),
  });
  const tabRefs = useRef<Partial<Record<DetailTab, HTMLButtonElement | null>>>({});
  const request = detail.request;
  const response = detail.response;
  const [origin, path] = recordDetailUrl(request);
  const panelId = `record-panel-${request.id}`;

  function selectAdjacentTab(event: React.KeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % TABS.length;
    if (event.key === "ArrowLeft") nextIndex = (index - 1 + TABS.length) % TABS.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = TABS.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    const next = TABS[nextIndex].value;
    onTabChange(next);
    tabRefs.current[next]?.focus();
  }

  return (
    <section className={styles.panel} aria-label="Traffic record details">
      <div className={styles.header}>
        <div className={styles.requestOverview}>
          <span className={styles.method}>{request.method}</span>
          <span className={styles.url}>
            <strong>{origin}</strong>
            <span>{path}</span>
          </span>
        </div>
        <RecordHeadlineStatus
          response={response}
          state={detail.state}
          assessment={detail.assessment}
        />
      </div>
      <div className={styles.tabs} role="tablist" aria-label="Record data">
        {TABS.map(({ value, label }, index) => (
          <button
            ref={(element) => {
              tabRefs.current[value] = element;
            }}
            key={value}
            id={`record-tab-${request.id}-${value}`}
            type="button"
            role="tab"
            aria-controls={panelId}
            aria-selected={tab === value}
            tabIndex={tab === value ? 0 : -1}
            className={tab === value ? styles.activeTab : ""}
            onClick={() => onTabChange(value)}
            onKeyDown={(event) => selectAdjacentTab(event, index)}
          >
            {label}
          </button>
        ))}
      </div>
      <div
        id={panelId}
        className={styles.tabPanel}
        role="tabpanel"
        aria-labelledby={`record-tab-${request.id}-${tab}`}
      >
        {tab === "summary" ? (
          <Summary detail={detail} />
        ) : tab === "response" && !response ? (
          <div className={styles.noResponse}>
            <h2>No response received</h2>
            <p>The Traffic Record does not contain response metadata.</p>
          </div>
        ) : (
          <MessageData
            kind={tab}
            detail={detail}
            bodyChunks={bodies[tab]}
            bodyStatus={bodyStatus[tab]}
            decoded={decodedBodies[tab]}
            timings={tab === "response" ? eventTimings : null}
            loadingBody={loadingBody}
            memory={bodyViews[tab]}
            onMemoryChange={(memory) => setBodyViews((current) => ({ ...current, [tab]: memory }))}
            onDownload={() => onDownload(tab)}
          />
        )}
      </div>
    </section>
  );
}

function Summary({ detail }: { detail: RecordDetailData }) {
  const [copiedSessionId, setCopiedSessionId] = useState<string | null>(null);
  const sessionCopiedTimer = useRef<number | undefined>(undefined);
  const total = detail.result?.total_ms ?? detail.live_total_ms;
  const protocol = detail.summary.protocol;
  const model =
    resolveRequestedEffective(protocol?.model) ??
    (detail.state === "active" ? "Detecting…" : "Not reported");
  const reasoningEffort = resolveRequestedEffective(protocol?.reasoning_effort);
  const sessionId = detail.summary.coding_agent_session_id;
  const sessionCopied = sessionId !== null && copiedSessionId === sessionId;
  const stages = timingStages(detail);
  const firstToken = elapsedNsMs(protocol?.first_token_at_ns);
  const mode = protocol?.response_mode.observed ?? protocol?.response_mode.requested;
  const responseMode = mode === "stream" ? "Streaming" : mode === "normal" ? "Non-streaming" : null;
  const diagnostics = detail.diagnostics;
  const hasDiagnostics =
    diagnostics.traffic.length > 0 ||
    diagnostics.http.length > 0 ||
    diagnostics.provider.length > 0 ||
    diagnostics.warnings.length > 0;

  useEffect(
    () => () => {
      if (sessionCopiedTimer.current !== undefined) {
        window.clearTimeout(sessionCopiedTimer.current);
      }
    },
    [],
  );

  async function copySessionId() {
    if (!sessionId) return;
    try {
      await navigator.clipboard.writeText(sessionId);
      setCopiedSessionId(sessionId);
      if (sessionCopiedTimer.current !== undefined) {
        window.clearTimeout(sessionCopiedTimer.current);
      }
      sessionCopiedTimer.current = window.setTimeout(() => setCopiedSessionId(null), 1400);
    } catch {
      setCopiedSessionId(null);
    }
  }

  return (
    <div className={styles.summary}>
      <section className={styles.modelSummary} aria-labelledby="record-model-title">
        <h2 id="record-model-title">Model</h2>
        <div className={styles.modelHeadline}>
          <p className={styles.modelName} title={`Model ${model}`}>
            <span className={styles.modelValue}>{model}</span>
            {reasoningEffort && (
              <>
                <span className={styles.modelSeparator} aria-hidden="true">
                  ·
                </span>
                <span className={styles.modelEffort}>{reasoningEffort}</span>
              </>
            )}
          </p>
          {responseMode && <span className={styles.modeBadge}>{responseMode}</span>}
        </div>
        <dl className={styles.sessionMeta}>
          <div className={styles.sessionFact}>
            <dt>Session ID</dt>
            <dd>
              <span className={styles.sessionValue}>{sessionId ?? "Not reported"}</span>
              {sessionId && (
                <button
                  className={styles.copySession}
                  type="button"
                  onClick={() => void copySessionId()}
                  aria-label={sessionCopied ? "Session ID copied" : "Copy Session ID"}
                  title={sessionCopied ? "Session ID copied" : "Copy Session ID"}
                >
                  {sessionCopied ? (
                    <Check size={14} aria-hidden="true" />
                  ) : (
                    <Clipboard size={14} aria-hidden="true" />
                  )}
                </button>
              )}
            </dd>
          </div>
        </dl>
        <TokenUsageGroup detail={detail} usage={protocol?.token_usage ?? null} />
      </section>
      <section aria-labelledby="record-timing-title">
        <h2 id="record-timing-title">Timing</h2>
        <dl className={`${styles.metricGrid} ${styles.timingMetrics}`}>
          <Metric label="First token" value={duration(firstToken)} />
          <Metric label="Duration" value={duration(total)} />
          <Metric label="Ended" value={formatTimestamp(detail.result?.ended_at ?? "")} />
        </dl>
        {stages.length > 0 ? (
          <div className={styles.timeline} role="list" aria-label="Timing stages">
            {stages.map((stage) => {
              const status = stage.status === "complete" ? "" : ` · ${stage.status}`;
              const value = `${duration(stage.durationMs)}${status}`;
              const style = {
                "--stage-start": `${stage.startPercent}%`,
                "--stage-width": `${stage.widthPercent}%`,
              } as CSSProperties;
              return (
                <div
                  key={stage.label}
                  className={styles.timelineRow}
                  role="listitem"
                  aria-label={`${stage.label}: ${value}`}
                >
                  <span className={styles.timelineLabel}>{stage.label}</span>
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
        ) : (
          <p className={styles.sectionState}>Timing stages are not available yet.</p>
        )}
      </section>
      <section className={styles.diagnostics} aria-labelledby="record-diagnostics-title">
        <h2 id="record-diagnostics-title">Diagnostics</h2>
        {hasDiagnostics ? (
          <div className={styles.diagnosticGroups}>
            <DiagnosticGroup title="Proxy / transport" entries={diagnostics.traffic} tone="error" />
            <DiagnosticGroup title="HTTP response" entries={diagnostics.http} tone="error" />
            <DiagnosticGroup title="Model API" entries={diagnostics.provider} tone="error" />
            <DiagnosticGroup title="Warnings" entries={diagnostics.warnings} tone="warning" />
          </div>
        ) : (
          <p className={styles.sectionState}>No diagnostics.</p>
        )}
      </section>
    </div>
  );
}

function TokenUsageGroup({
  detail,
  usage,
}: {
  detail: RecordDetailData;
  usage: TokenUsage | null;
}) {
  const protocol = detail.summary.protocol;
  const state = usageState(detail);
  const claude = protocol?.family === "claude_messages";
  const hasCacheWriteBreakdown =
    claude && (usage?.cache_write_5m_tokens != null || usage?.cache_write_1h_tokens != null);
  const cacheWrites = hasCacheWriteBreakdown
    ? (usage?.cache_write_5m_tokens ?? 0) + (usage?.cache_write_1h_tokens ?? 0)
    : (usage?.cache_write_tokens ?? null);
  const inputMetrics: Array<{
    label: string;
    value: number | null;
    details?: Array<{ label: string; value: number | null }>;
  }> = [
    {
      label: claude ? "Base input" : "Input",
      value: usage?.base_input_tokens ?? null,
    },
    {
      label: claude ? "Cache hits & refreshes" : "Cached input",
      value: usage?.cached_input_tokens ?? null,
    },
    {
      label: "Cache writes",
      value: cacheWrites,
      details: hasCacheWriteBreakdown
        ? [
            { label: "5m", value: usage?.cache_write_5m_tokens ?? null },
            { label: "1h", value: usage?.cache_write_1h_tokens ?? null },
          ]
        : undefined,
    },
  ];
  const totalInput = usage?.total_input_tokens ?? null;
  const output = usage?.output_tokens ?? null;
  const reasoning = output !== null ? (usage?.reasoning_output_tokens ?? null) : null;
  const hasUsageData = [
    usage?.total_input_tokens,
    usage?.base_input_tokens,
    usage?.cached_input_tokens,
    usage?.cache_write_tokens,
    usage?.cache_write_5m_tokens,
    usage?.cache_write_1h_tokens,
    usage?.output_tokens,
  ].some((value) => value != null);
  return (
    <section className={styles.usageGroup} aria-labelledby="record-token-title">
      <div className={styles.usageHeading}>
        <h3 id="record-token-title">Token usage</h3>
      </div>
      {hasUsageData ? (
        <div className={styles.tokenUsageGrid}>
          <div className={styles.tokenInputBlock}>
            <div className={styles.tokenInputMetrics} role="group" aria-label="Input tokens">
              {inputMetrics.map((metric) => (
                <div
                  className={`${styles.tokenInputCell} ${
                    metric.details ? styles.tokenInputCellDetailed : ""
                  }`}
                  role="group"
                  aria-label={`${metric.label} billing category`}
                  key={metric.label}
                >
                  <dl className={styles.tokenInputPrimary}>
                    <div>
                      <dt>{metric.label}</dt>
                      <dd>{displayTokenCount(metric.value)}</dd>
                    </div>
                  </dl>
                  {metric.details && (
                    <dl
                      className={styles.tokenCacheBreakdown}
                      role="group"
                      aria-label="Cache write TTL breakdown"
                    >
                      {metric.details.map((detailMetric) => (
                        <div className={styles.tokenCacheDetail} key={detailMetric.label}>
                          <dt>{detailMetric.label}</dt>
                          <dd>{displayTokenCount(detailMetric.value)}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                </div>
              ))}
            </div>
            <dl className={styles.tokenTotal} role="group" aria-label="Total input tokens">
              <div>
                <dt>Total input</dt>
                <dd>{displayTokenCount(totalInput)}</dd>
              </div>
            </dl>
          </div>
          <dl className={styles.tokenOutput} role="group" aria-label="Output tokens">
            <div className={styles.tokenOutputPrimary}>
              <dt>Output</dt>
              <dd>{displayTokenCount(output)}</dd>
            </div>
            {reasoning !== null && (
              <div
                className={styles.tokenReasoning}
                role="group"
                aria-label={`Output includes ${tokenCount(reasoning)} reasoning tokens`}
              >
                <dt>Reasoning</dt>
                <dd>{tokenCount(reasoning)}</dd>
              </div>
            )}
          </dl>
        </div>
      ) : (
        <p className={styles.usageMessage}>{usageStateMessage(state)}</p>
      )}
    </section>
  );
}

function DiagnosticGroup({
  title,
  entries,
  tone,
}: {
  title: string;
  entries: AssessmentFinding[];
  tone: "error" | "warning";
}) {
  if (entries.length === 0) return null;
  return (
    <section
      className={`${styles.diagnosticGroup} ${
        tone === "error" ? styles.errorDiagnostics : styles.warningDiagnostics
      }`}
      aria-label={title}
    >
      <h3>
        {title} <span>{entries.length || 1}</span>
      </h3>
      <div className={styles.diagnosticList}>
        {entries.map((entry, index) => (
          <article
            className={styles.diagnosticItem}
            key={`${entry.source}-${entry.kind}-${entry.at_ns}-${index}`}
          >
            <div className={styles.diagnosticMeta}>
              <strong>{assessmentPrimaryLabel(entry)}</strong>
              {entry.phase && <span>{entry.phase}</span>}
              {entry.at_ns && <span>{duration(elapsedNsMs(entry.at_ns))}</span>}
            </div>
            <p>{entry.message}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

function usageState(detail: RecordDetailData): UsageState {
  const protocol = detail.summary.protocol;
  if (!protocol || protocol.family === "unknown") return "unsupported";
  if (protocol.token_usage) return "final";
  if (detail.state === "active" && !protocol.response_terminal) return "waiting";
  return "not_reported";
}

function usageStateMessage(state: UsageState): string {
  return {
    waiting: "Waiting for the upstream API to report token usage.",
    final: "The upstream API reported no token counters.",
    not_reported: "The completed response did not report token usage.",
    unsupported: "Token usage is unavailable for this protocol.",
  }[state];
}

function displayTokenCount(value: number | null): string {
  return value === null ? "—" : tokenCount(value);
}

function capitalize(value: string): string {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}

function MessageData({
  kind,
  detail,
  bodyChunks,
  bodyStatus,
  decoded,
  timings,
  loadingBody,
  memory,
  onMemoryChange,
  onDownload,
}: {
  kind: BodyKind;
  detail: RecordDetailData;
  bodyChunks: Uint8Array[];
  bodyStatus: BodyLoadStatus;
  decoded: DecodedBodyState;
  timings: EventTimingIndex | null;
  loadingBody: boolean;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
  onDownload: () => void;
}) {
  const headers = kind === "request" ? detail.request.headers : (detail.response?.headers ?? []);

  return (
    <div className={styles.messageData}>
      <div className={styles.sectionTitle}>
        <h2>
          <FileText size={15} aria-hidden="true" /> Headers
        </h2>
      </div>
      {headers.length > 0 ? (
        <table className={styles.headers}>
          <caption className="srOnly">{kind} headers</caption>
          <tbody>
            {headers.map((header, index) => (
              <tr key={`${header.name}-${index}`}>
                <td>{header.name}</td>
                <td>{decodeHeader(header)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className={styles.empty}>No headers.</p>
      )}
      <BodyViewer
        kind={kind}
        detail={detail}
        bodyChunks={bodyChunks}
        bodyStatus={bodyStatus}
        decoded={decoded}
        timings={timings}
        loadingBody={loadingBody}
        memory={memory}
        onMemoryChange={onMemoryChange}
        onDownload={onDownload}
      />
    </div>
  );
}

function Metric({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className={styles.metric}>
      <dt>{label}</dt>
      <dd>
        <span className={styles.metricValue}>{value}</span>
        {detail && <span className={styles.metricDetail}>{detail}</span>}
      </dd>
    </div>
  );
}
