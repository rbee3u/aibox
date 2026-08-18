import type { RecordAssessment, RecordState, ResponseMetadata } from "../types";
import styles from "./RecordStatus.module.css";
import { IssueIndicator, IssueTooltip } from "./IssueIndicator";
import {
  assessmentIssueText,
  recordHeadlinePresentation,
  recordStatusPresentation,
  type AssessmentPresentation,
  type RecordStatusTone,
} from "./statusPresentation";

interface RecordStatusProps {
  status: number | null;
  state: RecordState;
  assessment: RecordAssessment;
}

const TONE_CLASS: Record<RecordStatusTone, string> = {
  active: styles.active,
  error: styles.error,
  neutral: styles.neutral,
  success: styles.success,
  warning: styles.warningTone,
};

export function RecordStatus({ status, state, assessment }: RecordStatusProps) {
  const presentation = recordStatusPresentation({ status, state, assessment });

  return (
    <span className={styles.root}>
      <span className={`${styles.value} ${TONE_CLASS[presentation.tone]}`}>
        {presentation.tone === "active" && <span className={styles.dot} aria-hidden="true" />}
        {presentation.label}
      </span>
      {presentation.phase && (
        <span className={styles.phase}>
          <span className={styles.dot} aria-hidden="true" /> {presentation.phase}
        </span>
      )}
      {presentation.issue && <CompactIssue issue={presentation.issue} />}
    </span>
  );
}

function CompactIssue({ issue }: { issue: AssessmentPresentation }) {
  return (
    <IssueIndicator
      tone={issue.tone}
      label={issue.label}
      message={issue.message}
      ariaLabel={assessmentIssueText(issue)}
    />
  );
}

interface RecordHeadlineStatusProps {
  response: ResponseMetadata | null;
  state: RecordState;
  assessment: RecordAssessment;
}

export function RecordHeadlineStatus({ response, state, assessment }: RecordHeadlineStatusProps) {
  const presentation = recordHeadlinePresentation(response, state, assessment);
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
      {presentation.tag && "message" in presentation.tag ? (
        <IssueTooltip
          tone={presentation.tag.tone}
          label={presentation.tag.label}
          message={presentation.tag.message}
          className={`${styles.tag} ${
            presentation.tag.tone === "warning" ? styles.warningTag : styles.errorTag
          }`}
        >
          <span className={styles.tagLabel}>{presentation.tag.label}</span>
          {presentation.tag.additionalIssues > 0 && (
            <span className={styles.issueCount}>+{presentation.tag.additionalIssues}</span>
          )}
        </IssueTooltip>
      ) : presentation.tag ? (
        <span className={`${styles.tag} ${styles.activeTag}`}>
          <span className={styles.dot} aria-hidden="true" />
          <span className={styles.tagLabel}>{presentation.tag.label}</span>
        </span>
      ) : null}
    </div>
  );
}
