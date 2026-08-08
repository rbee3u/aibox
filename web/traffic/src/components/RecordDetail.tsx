import { Check, Clipboard, Download, FileText, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type {
  BodyKind,
  BodyLoadStatus,
  DetailTab,
  ProtocolDiagnostic,
  RecordDetail as RecordDetailData,
  SummaryDiagnostic,
  TokenUsage,
  UsageState,
} from "../types";
import { elapsedNsMs, resolveRequestedEffective, timingStages, tokenCount } from "../summary";
import {
  bytes,
  concatChunks,
  decodeBytes,
  decodeHeader,
  duration,
  formatTimestamp,
  recordDetailUrl,
} from "../utils";
import styles from "./RecordDetail.module.css";
import { RecordHeadlineStatus } from "./RecordStatus";
import { errorKindLabel } from "./statusPresentation";

const TABS: Array<{ value: DetailTab; label: string }> = [
  { value: "summary", label: "Summary" },
  { value: "request", label: "Request" },
  { value: "response", label: "Response" },
];

interface RecordDetailProps {
  detail: RecordDetailData;
  bodies: Record<BodyKind, Uint8Array[]>;
  bodyStatus: Record<BodyKind, BodyLoadStatus>;
  tab: DetailTab;
  onTabChange: (tab: DetailTab) => void;
  onDownload: (kind: BodyKind) => void;
  loadingBody: boolean;
}

export function RecordDetail({
  detail,
  bodies,
  bodyStatus,
  tab,
  onTabChange,
  onDownload,
  loadingBody,
}: RecordDetailProps) {
  const [copiedKind, setCopiedKind] = useState<BodyKind | null>(null);
  const copiedTimer = useRef<number | undefined>(undefined);
  const tabRefs = useRef<Partial<Record<DetailTab, HTMLButtonElement | null>>>({});
  const request = detail.request;
  const response = detail.response;
  const result = detail.result;
  const [origin, path] = recordDetailUrl(request);
  const panelId = `record-panel-${request.id}`;

  useEffect(
    () => () => {
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  async function copyBody(kind: BodyKind) {
    const bodyText = decodeBytes(concatChunks(bodies[kind]), "body");
    try {
      await navigator.clipboard.writeText(bodyText);
      setCopiedKind(kind);
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopiedKind(null), 1400);
    } catch {
      setCopiedKind(null);
    }
  }

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
        <RecordHeadlineStatus response={response} result={result} state={detail.state} />
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
            loadingBody={loadingBody}
            copied={copiedKind === tab}
            onCopy={() => void copyBody(tab)}
            onDownload={() => onDownload(tab)}
          />
        )}
      </div>
    </section>
  );
}

function Summary({ detail }: { detail: RecordDetailData }) {
  const total = detail.result?.total_ms ?? detail.live_total_ms;
  const protocol = detail.summary.protocol;
  const resolvedModel = resolveRequestedEffective(protocol?.model);
  const resolvedReasoningEffort = resolveRequestedEffective(protocol?.reasoning_effort);
  const model = resolvedModel.value ?? (detail.state === "active" ? "Detecting…" : "Not reported");
  const reasoningEffort = resolvedReasoningEffort.value ?? "—";
  const modelSources = [
    resolvedModel.source ? `${capitalize(resolvedModel.source)} model` : null,
    resolvedReasoningEffort.source
      ? `${capitalize(resolvedReasoningEffort.source)} reasoning effort`
      : null,
  ].filter((source): source is string => source !== null);
  const stages = timingStages(detail);
  const firstToken = elapsedNsMs(protocol?.first_token_at_ns);
  const mode = protocol?.response_mode.requested ?? protocol?.response_mode.observed ?? "unknown";
  const trafficErrors = detail.summary.errors;
  const trafficWarnings = detail.summary.warnings;
  const protocolErrors = protocol?.errors ?? [];
  const protocolWarnings = protocol?.warnings ?? [];
  const hasDiagnostics =
    protocolErrors.length > 0 ||
    trafficErrors.length > 0 ||
    trafficWarnings.length > 0 ||
    protocolWarnings.length > 0 ||
    detail.state === "interrupted";
  return (
    <div className={styles.summary}>
      <section className={styles.modelSummary} aria-labelledby="record-model-title">
        <div className={styles.modelTitleRow}>
          <h2 id="record-model-title">Model</h2>
          <span className={`${styles.modeBadge} ${styles[`mode${capitalize(mode)}`]}`}>{mode}</span>
        </div>
        <p
          className={styles.modelName}
          title={`Model ${model}; Reasoning effort ${reasoningEffort}`}
        >
          <span>{model}</span>
          <span className={styles.modelSeparator} aria-hidden="true">
            ·
          </span>
          <span>{reasoningEffort}</span>
        </p>
        {modelSources.length > 0 && (
          <p className={styles.modelSource}>{modelSources.join(" · ")}</p>
        )}
      </section>
      <section aria-labelledby="record-timing-title">
        <h2 id="record-timing-title">Timing</h2>
        <dl className={styles.metrics}>
          <Metric label="First token" value={duration(firstToken)} />
          <Metric label="Duration" value={duration(total)} />
          <Metric label="Started" value={formatTimestamp(detail.request.started_at)} />
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
      <TokenUsageSection detail={detail} usage={protocol?.token_usage ?? null} />
      <section className={styles.diagnostics} aria-labelledby="record-diagnostics-title">
        <h2 id="record-diagnostics-title">Diagnostics</h2>
        {hasDiagnostics ? (
          <div className={styles.diagnosticGroups}>
            <DiagnosticGroup title="API / Provider errors" entries={protocolErrors} />
            <DiagnosticGroup
              title="Traffic errors"
              entries={trafficErrors}
              fallback={
                detail.state === "interrupted" && trafficErrors.length === 0
                  ? "The Traffic Record ended without a terminal diagnostic."
                  : undefined
              }
            />
            <DiagnosticGroup title="Warnings" entries={[...trafficWarnings, ...protocolWarnings]} />
          </div>
        ) : (
          <p className={styles.sectionState}>No diagnostics.</p>
        )}
      </section>
    </div>
  );
}

function TokenUsageSection({
  detail,
  usage,
}: {
  detail: RecordDetailData;
  usage: TokenUsage | null;
}) {
  const protocol = detail.summary.protocol;
  const state = usageState(detail);
  const metrics: Array<{ label: string; value: number | null }> = [];
  if (usage) {
    metrics.push({ label: "Total input", value: usage.total_input_tokens });
    if (protocol?.family === "claude_messages") {
      metrics.push(
        { label: "Base Input Tokens", value: usage.base_input_tokens },
        { label: "Cache Hits & Refreshes", value: usage.cached_input_tokens },
      );
    } else {
      metrics.push(
        { label: "Base input", value: usage.base_input_tokens },
        { label: "Cached input", value: usage.cached_input_tokens },
      );
    }
    if (usage.cache_write_5m_tokens !== null || usage.cache_write_1h_tokens !== null) {
      metrics.push(
        { label: "5m Cache Writes", value: usage.cache_write_5m_tokens },
        { label: "1h Cache Writes", value: usage.cache_write_1h_tokens },
      );
    } else {
      metrics.push({ label: "Cache writes", value: usage.cache_write_tokens });
    }
    metrics.push({
      label: protocol?.family === "claude_messages" ? "Output Tokens" : "Output",
      value: usage.output_tokens,
    });
    metrics.push({ label: "Reasoning output", value: usage.reasoning_output_tokens });
  }
  const visible = metrics.filter((metric) => metric.value !== null);
  return (
    <section aria-labelledby="record-token-title">
      <div className={styles.sectionHeading}>
        <h2 id="record-token-title">Token Usage</h2>
        <span className={`${styles.usageState} ${styles[`usage${capitalize(state)}`]}`}>
          {usageStateLabel(state)}
        </span>
      </div>
      {visible.length > 0 ? (
        <dl className={styles.tokenMetrics}>
          {visible.map((metric) => (
            <Metric key={metric.label} label={metric.label} value={tokenCount(metric.value!)} />
          ))}
        </dl>
      ) : (
        <p className={styles.sectionState}>{usageStateMessage(state)}</p>
      )}
    </section>
  );
}

type DiagnosticEntry = SummaryDiagnostic | ProtocolDiagnostic;

function DiagnosticGroup({
  title,
  entries,
  fallback,
}: {
  title: string;
  entries: DiagnosticEntry[];
  fallback?: string;
}) {
  if (entries.length === 0 && !fallback) return null;
  return (
    <section className={styles.diagnosticGroup} aria-label={title}>
      <h3>
        {title} <span>{entries.length || 1}</span>
      </h3>
      <div className={styles.diagnosticList}>
        {fallback && entries.length === 0 ? (
          <article className={styles.diagnosticItem}>
            <strong>Interrupted</strong>
            <p>{fallback}</p>
          </article>
        ) : (
          entries.map((entry, index) => (
            <article
              className={styles.diagnosticItem}
              key={`${entry.kind}-${entry.at_ns}-${index}`}
            >
              <div className={styles.diagnosticMeta}>
                <strong>{errorKindLabel(entry.kind)}</strong>
                {"phase" in entry && <span>{entry.phase}</span>}
                {entry.at_ns && <span>{duration(elapsedNsMs(entry.at_ns))}</span>}
              </div>
              <p>{entry.message}</p>
            </article>
          ))
        )}
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

function usageStateLabel(state: UsageState): string {
  return {
    waiting: "Waiting",
    final: "Final",
    not_reported: "Not reported",
    unsupported: "Unsupported",
  }[state];
}

function usageStateMessage(state: UsageState): string {
  return {
    waiting: "Waiting for the upstream API to report token usage.",
    final: "The upstream API reported no token counters.",
    not_reported: "The completed response did not report token usage.",
    unsupported: "Token usage is unavailable for this protocol.",
  }[state];
}

function capitalize(value: string): string {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : value;
}

function MessageData({
  kind,
  detail,
  bodyChunks,
  bodyStatus,
  loadingBody,
  copied,
  onCopy,
  onDownload,
}: {
  kind: BodyKind;
  detail: RecordDetailData;
  bodyChunks: Uint8Array[];
  bodyStatus: BodyLoadStatus;
  loadingBody: boolean;
  copied: boolean;
  onCopy: () => void;
  onDownload: () => void;
}) {
  const headers = kind === "request" ? detail.request.headers : (detail.response?.headers ?? []);
  const bodyBytes = concatChunks(bodyChunks);
  const bodyText = decodeBytes(bodyBytes, "body");
  const recordedBytes = kind === "request" ? detail.request_body_bytes : detail.response_body_bytes;
  const loaded = bodyStatus === "loaded";

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
      <div className={styles.sectionTitle}>
        <h2>
          Body <span>· {bytes(recordedBytes)}</span>
        </h2>
        <div className={styles.bodyActions}>
          {loadingBody && (
            <LoaderCircle className={styles.loading} size={15} aria-label="Loading body" />
          )}
          <button
            type="button"
            onClick={onCopy}
            disabled={!loaded}
            aria-label={copied ? "Body copied" : "Copy body"}
            title={copied ? "Body copied" : "Copy body"}
          >
            {copied ? (
              <Check size={15} aria-hidden="true" />
            ) : (
              <Clipboard size={15} aria-hidden="true" />
            )}
          </button>
          <button
            type="button"
            onClick={onDownload}
            aria-label="Download original body"
            title="Download original body"
          >
            <Download size={15} aria-hidden="true" />
          </button>
        </div>
      </div>
      {bodyStatus === "error" ? (
        <p className={styles.bodyState}>Body unavailable.</p>
      ) : bodyStatus === "idle" || bodyStatus === "loading" ? (
        <div className={styles.bodyState} role="status">
          <LoaderCircle className={styles.loading} size={16} aria-hidden="true" /> Loading body…
        </div>
      ) : (
        <pre className={styles.body}>{bodyText || "(empty body)"}</pre>
      )}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.metric}>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}
