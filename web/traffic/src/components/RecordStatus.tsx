import { TriangleAlert } from "lucide-react";
import type { RecordState, ResponseMetadata, ResultMetadata } from "../types";
import styles from "./RecordStatus.module.css";
import {
  recordHeadlinePresentation,
  recordStatusPresentation,
  type RecordStatusTone,
} from "./statusPresentation";

interface RecordStatusProps {
  status: number | null;
  httpVersion?: string | null;
  outcome: string;
  state: RecordState;
  compact?: boolean;
}

const TONE_CLASS: Record<RecordStatusTone, string> = {
  active: styles.active,
  error: styles.error,
  neutral: styles.neutral,
  success: styles.success,
};

export function RecordStatus({
  status,
  httpVersion,
  outcome,
  state,
  compact = false,
}: RecordStatusProps) {
  const presentation = recordStatusPresentation({ status, outcome, state });
  const anomalyTitle = presentation.anomaly ? `Record outcome: ${presentation.anomaly}` : undefined;
  const noResponse = status === null && state !== "active";

  return (
    <span className={styles.root}>
      {httpVersion && status !== null && <span className={styles.protocol}>{httpVersion}</span>}
      <span
        className={`${styles.value} ${TONE_CLASS[presentation.tone]}`}
        title={noResponse ? anomalyTitle : undefined}
      >
        {presentation.tone === "active" && <span className={styles.dot} aria-hidden="true" />}
        {presentation.label}
        {compact && noResponse && anomalyTitle && <span className="srOnly">. {anomalyTitle}</span>}
      </span>
      {presentation.phase && (
        <span className={styles.phase}>
          <span className={styles.dot} aria-hidden="true" /> {presentation.phase}
        </span>
      )}
      {presentation.anomaly &&
        !noResponse &&
        (compact ? (
          <span
            className={styles.warning}
            role="img"
            aria-label={anomalyTitle}
            title={anomalyTitle}
          >
            <TriangleAlert size={12} strokeWidth={2.2} aria-hidden="true" />
          </span>
        ) : (
          <span className={styles.anomaly}>{presentation.anomaly}</span>
        ))}
      {presentation.anomaly && noResponse && !compact && (
        <span className={styles.anomaly}>{presentation.anomaly}</span>
      )}
    </span>
  );
}

interface RecordHeadlineStatusProps {
  response: ResponseMetadata | null;
  result: ResultMetadata | null;
  state: RecordState;
}

export function RecordHeadlineStatus({ response, result, state }: RecordHeadlineStatusProps) {
  const presentation = recordHeadlinePresentation(response, result, state);
  const visualStatusText = response
    ? [response.status, response.reason_phrase].filter(Boolean).join(" ")
    : presentation.statusText;

  return (
    <div className={styles.headline}>
      {presentation.statusText && (
        <span
          className={`${styles.headlineStatus} ${TONE_CLASS[presentation.tone]}`}
          aria-label={presentation.statusText}
        >
          {response?.http_version && (
            <span className={styles.protocol}>{response.http_version}</span>
          )}
          <span>{visualStatusText}</span>
        </span>
      )}
      {presentation.tag && presentation.tagTone && (
        <span
          className={`${styles.tag} ${
            presentation.tagTone === "active" ? styles.activeTag : styles.errorTag
          }`}
        >
          {presentation.tagTone === "active" && <span className={styles.dot} aria-hidden="true" />}
          {presentation.tag}
        </span>
      )}
    </div>
  );
}
