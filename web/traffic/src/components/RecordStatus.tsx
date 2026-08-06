import { TriangleAlert } from "lucide-react";
import type { RecordState } from "../types";
import styles from "./RecordStatus.module.css";
import { recordStatusPresentation, type RecordStatusTone } from "./statusPresentation";

interface RecordStatusProps {
  status: number | null;
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

export function RecordStatus({ status, outcome, state, compact = false }: RecordStatusProps) {
  const presentation = recordStatusPresentation({ status, outcome, state });
  const anomalyTitle = presentation.anomaly ? `Record outcome: ${presentation.anomaly}` : undefined;
  const noResponse = status === null && state !== "active";

  return (
    <span className={styles.root}>
      <span
        className={`${styles.value} ${TONE_CLASS[presentation.tone]}`}
        title={noResponse ? anomalyTitle : undefined}
      >
        {presentation.tone === "active" && <span className={styles.dot} aria-hidden="true" />}
        {presentation.label}
        {compact && noResponse && anomalyTitle && <span className="srOnly">. {anomalyTitle}</span>}
      </span>
      {presentation.recording &&
        (compact ? (
          <span
            className={styles.compactRecording}
            role="img"
            aria-label="Recording active traffic"
            title="Recording active traffic"
          >
            <span className={styles.dot} aria-hidden="true" />
          </span>
        ) : (
          <span className={styles.recording}>
            <span className={styles.dot} aria-hidden="true" /> recording
          </span>
        ))}
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
