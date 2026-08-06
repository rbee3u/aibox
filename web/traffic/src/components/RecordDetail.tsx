import { Check, Clipboard, Download, FileText, LoaderCircle } from "lucide-react";
import { useMemo, useState } from "react";
import type { RecordDetail as RecordDetailData } from "../types";
import {
  bytes,
  concatChunks,
  decodeBytes,
  decodeHeader,
  duration,
  formatTimestamp,
  queryParams,
} from "../utils";
import styles from "./RecordDetail.module.css";
import { RecordStatus } from "./RecordStatus";

interface RecordDetailProps {
  detail: RecordDetailData;
  bodies: { request: Uint8Array[]; response: Uint8Array[] };
  tab: "request" | "response";
  onTabChange: (tab: "request" | "response") => void;
  onDownload: (kind: "request" | "response") => void;
  loadingBody: boolean;
}

export function RecordDetail({
  detail,
  bodies,
  tab,
  onTabChange,
  onDownload,
  loadingBody,
}: RecordDetailProps) {
  const [copied, setCopied] = useState(false);
  const request = detail.request;
  const response = detail.response;
  const result = detail.result;
  const bodyBytes = concatChunks(bodies[tab]);
  const bodyText = decodeBytes(bodyBytes, "body");
  const headers = tab === "request" ? request.headers : (response?.headers ?? []);
  const params = useMemo(() => queryParams(request.upstream_url), [request.upstream_url]);

  async function copyBody() {
    try {
      await navigator.clipboard.writeText(bodyText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  }

  return (
    <section className={styles.panel} aria-label="Traffic record details">
      <div className={styles.header}>
        <div className={styles.eyebrow}>
          <span className={styles.method}>{request.method}</span>
          <RecordStatus
            status={response?.status ?? null}
            outcome={result?.outcome ?? detail.state}
            state={detail.state}
          />
        </div>
        <div className={styles.url}>{request.upstream_url ?? request.incoming_uri}</div>
        <div className={styles.chips}>
          <span className={styles.chip}>{request.http_version}</span>
          <span className={styles.chip}>{response?.source ?? "no response"}</span>
        </div>
        {params.length > 0 && (
          <table className={styles.query}>
            <caption>Query parameters</caption>
            <tbody>
              {params.map(([key, value], index) => (
                <tr key={`${key}-${index}`}>
                  <td>{key}</td>
                  <td>{value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <div className={styles.metrics}>
          <Metric label="Started" value={formatTimestamp(request.started_at)} />
          <Metric
            label="Response headers"
            value={response?.headers_at ? new Date(response.headers_at).toLocaleTimeString() : "—"}
          />
          <Metric
            label="Ended"
            value={result?.ended_at ? new Date(result.ended_at).toLocaleTimeString() : "—"}
          />
          <Metric label="TTFB" value={duration(result?.ttfb_ms ?? detail.live_ttfb_ms)} />
          <Metric label="Total" value={duration(result?.total_ms ?? detail.live_total_ms)} />
          <Metric label="Request body" value={bytes(detail.request_body_bytes)} />
          <Metric label="Response body" value={bytes(detail.response_body_bytes)} />
        </div>
      </div>
      <div className={styles.tabs} role="tablist" aria-label="Record data">
        {(["request", "response"] as const).map((value) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-selected={tab === value}
            className={tab === value ? styles.activeTab : ""}
            onClick={() => onTabChange(value)}
          >
            {value === "request" ? "Request" : "Response"}
          </button>
        ))}
      </div>
      <div className={styles.viewerTools}>
        <h2>
          <FileText size={15} aria-hidden="true" /> Headers
        </h2>
        <div>
          <button type="button" onClick={() => void copyBody()} aria-label="Copy body">
            {copied ? (
              <Check size={14} aria-hidden="true" />
            ) : (
              <Clipboard size={14} aria-hidden="true" />
            )}
            {copied ? "Copied" : "Copy body"}
          </button>
          <button type="button" onClick={() => onDownload(tab)} aria-label="Download original body">
            <Download size={14} aria-hidden="true" /> Download original
          </button>
        </div>
      </div>
      {headers.length > 0 ? (
        <table className={styles.headers}>
          <caption className="srOnly">{tab} headers</caption>
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
      <div className={styles.viewerTools}>
        <h2>
          Body <span>· {bytes(bodyBytes.length)}</span>
        </h2>
        {loadingBody && (
          <LoaderCircle className={styles.loading} size={15} aria-label="Loading body" />
        )}
      </div>
      <pre className={styles.body}>{bodyText || "(empty body)"}</pre>
    </section>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.metric}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
