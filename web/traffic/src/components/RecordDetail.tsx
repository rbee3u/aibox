import { Check, Clipboard, Download, FileText, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type {
  BodyKind,
  BodyLoadStatus,
  DetailTab,
  RecordDetail as RecordDetailData,
} from "../types";
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
import { recordErrorPresentation } from "./statusPresentation";

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
  const error = recordErrorPresentation(detail);
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
          <Summary detail={detail} error={error} />
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

function Summary({
  detail,
  error,
}: {
  detail: RecordDetailData;
  error: ReturnType<typeof recordErrorPresentation>;
}) {
  const total = detail.result?.total_ms ?? detail.live_total_ms;
  return (
    <div className={styles.summary}>
      <section aria-labelledby="record-timing-title">
        <h2 id="record-timing-title">Timing</h2>
        <dl className={styles.metrics}>
          <Metric label="Started" value={formatTimestamp(detail.request.started_at)} />
          <Metric label="First token" value="—" />
          <Metric label="Duration" value={duration(total)} />
        </dl>
      </section>
      {error && (
        <section className={styles.errorDetail} aria-labelledby="record-error-title">
          <h2 id="record-error-title">Error</h2>
          <dl>
            <div>
              <dt>Type</dt>
              <dd>{error.label}</dd>
            </div>
            <div>
              <dt>Message</dt>
              <dd>{error.message}</dd>
            </div>
          </dl>
        </section>
      )}
    </div>
  );
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
