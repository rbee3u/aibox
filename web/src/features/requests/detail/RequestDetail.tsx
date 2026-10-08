import styles from "@/features/requests/detail/RequestDetail.module.css";
import summarySharedStyles from "@/features/requests/detail/summaryShared.module.css";
import { RequestSummary } from "@/features/requests/detail/RequestSummary";
import { ChevronDown, ChevronRight, CircleOff, FileText } from "lucide-react";
import { useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type {
  BodyKind,
  EventTimingIndex,
  RequestDetail as RequestDetailData,
} from "@/api/requests";
import type { BodyLoadStatus, DecodedBodyState, DetailTab } from "@/features/requests/viewTypes";
import {
  bodyHeaders,
  headerListSummary,
  headerListToggleLabel,
} from "@/features/requests/detail/bodyPresentation";
import {
  createBodyViewMemory,
  type BodyViewMemory,
} from "@/features/requests/detail/bodyViewMemory";

import { compactDuration, formatTimestamp } from "@/shared/lib/format";
import { decodeHeader, requestDetailUrl } from "@/features/requests/requestFormat";
import { BodyViewer } from "@/features/requests/detail/BodyViewer";

import { RecordHeadlineStatus } from "@/features/requests/RequestStatus";

import { SegmentedControl } from "@/shared/ui/SegmentedControl";

import { EmptyState } from "@/shared/ui/EmptyState";
import { iconSize } from "@/shared/icons/iconSizes";

const TABS: Array<{ value: DetailTab; label: string }> = [
  { value: "summary", label: "Summary" },
  { value: "request", label: "Request" },
  { value: "response", label: "Response" },
];

interface RequestDetailProps {
  detail: RequestDetailData;
  bodies: Record<BodyKind, Uint8Array[]>;
  bodyStatus: Record<BodyKind, BodyLoadStatus>;
  decodedBodies: Record<BodyKind, DecodedBodyState>;
  eventTimings: EventTimingIndex | null;
  tab: DetailTab;
  onTabChange: (tab: DetailTab) => void;
  onDownload: (kind: BodyKind) => void;
  loadingBody: boolean;
}

export function RequestDetail({
  detail,
  bodies,
  bodyStatus,
  decodedBodies,
  eventTimings,
  tab,
  onTabChange,
  onDownload,
  loadingBody,
}: RequestDetailProps) {
  const [bodyViews, setBodyViews] = useState<Record<BodyKind, BodyViewMemory>>({
    request: createBodyViewMemory(),
    response: createBodyViewMemory(),
  });
  const tabRefs = useRef<Partial<Record<DetailTab, HTMLButtonElement | null>>>({});
  const request = detail.request;
  const response = detail.response;
  const [origin, path] = requestDetailUrl(request);
  const panelId = `request-panel-${request.id}`;
  const timestampKind = detail.result ? "Ended" : "Started";
  const timestampValue = detail.result?.ended_at ?? request.started_at;

  function selectAdjacentTab(event: KeyboardEvent<HTMLButtonElement>, index: number) {
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
    <section className={styles.panel} aria-label="Request details">
      <header className={styles.header}>
        <h2 className={styles.requestOverview} title={`${request.method} ${origin}${path}`}>
          <span className={styles.method}>{request.method}</span>
          <span className={styles.url}>
            <strong>{origin}</strong>
            <span>{path}</span>
          </span>
        </h2>
        <div className={styles.caption}>
          <RecordHeadlineStatus
            response={response}
            state={detail.state}
            assessment={detail.assessment}
          />
          {detail.result?.total_ms !== undefined && (
            <span className={styles.contextPill}>
              <small>Duration: </small>
              <strong>{compactDuration(detail.result.total_ms)}</strong>
            </span>
          )}
          <span className={styles.contextPill}>
            <span className={styles.captionTime}>
              {timestampKind}{" "}
              <time dateTime={timestampValue}>{formatTimestamp(timestampValue)}</time>
            </span>
          </span>
        </div>
      </header>
      <SegmentedControl variant="tabs" role="tablist" aria-label="Request data">
        {TABS.map(({ value, label }, index) => (
          <button
            ref={(element) => {
              tabRefs.current[value] = element;
            }}
            key={value}
            id={`request-tab-${request.id}-${value}`}
            type="button"
            role="tab"
            aria-controls={panelId}
            aria-selected={tab === value}
            tabIndex={tab === value ? 0 : -1}
            onClick={() => onTabChange(value)}
            onKeyDown={(event) => selectAdjacentTab(event, index)}
          >
            {label}
          </button>
        ))}
      </SegmentedControl>
      <div
        id={panelId}
        className={styles.tabPanel}
        role="tabpanel"
        aria-labelledby={`request-tab-${request.id}-${tab}`}
      >
        {tab === "summary" ? (
          <RequestSummary detail={detail} />
        ) : tab === "response" && !response ? (
          <EmptyState
            variant="detail"
            icon={<CircleOff size={iconSize.xl} aria-hidden="true" />}
            title="No response received"
            description="The Request does not contain response metadata."
          />
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
  detail: RequestDetailData;
  bodyChunks: Uint8Array[];
  bodyStatus: BodyLoadStatus;
  decoded: DecodedBodyState;
  timings: EventTimingIndex | null;
  loadingBody: boolean;
  memory: BodyViewMemory;
  onMemoryChange: (memory: BodyViewMemory) => void;
  onDownload: () => void;
}) {
  const headers = bodyHeaders(detail, kind);
  const headersOpen = memory.headersExpanded;
  const headerSummary = headerListSummary(headers);

  return (
    <div className={styles.messageData}>
      <div className={summarySharedStyles.sectionTitle}>
        <h2>
          {headers.length > 0 ? (
            <button
              type="button"
              className={styles.headersToggle}
              aria-expanded={headersOpen}
              aria-label={headerListToggleLabel(kind, headers)}
              onClick={() => onMemoryChange({ ...memory, headersExpanded: !headersOpen })}
            >
              {headersOpen ? (
                <ChevronDown size={iconSize.xs} aria-hidden="true" />
              ) : (
                <ChevronRight size={iconSize.xs} aria-hidden="true" />
              )}
              <FileText size={iconSize.xs} aria-hidden="true" /> Headers
              <span>{headerSummary}</span>
            </button>
          ) : (
            <>
              <FileText size={iconSize.xs} aria-hidden="true" /> Headers
            </>
          )}
        </h2>
      </div>
      {headers.length > 0 ? (
        headersOpen ? (
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
        ) : null
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
