import type { RequestAssessment, RequestState, ResponseMetadata } from "@/api/requests";
import styles from "@/features/requests/components/RequestStatus.module.css";
import { IssueIndicator, IssueTooltip } from "@/shared/ui/IssueIndicator";
import {
  assessmentIssueText,
  requestHeadlinePresentation,
  requestStatusPresentation,
  type AssessmentPresentation,
  type RequestStatusTone,
} from "@/features/requests/statusPresentation";

interface RequestStatusProps {
  status: number | null;
  state: RequestState;
  assessment: RequestAssessment;
}

const TONE_CLASS: Record<RequestStatusTone, string> = {
  active: styles.active,
  error: styles.error,
  neutral: styles.neutral,
  success: styles.success,
  warning: styles.warningTone,
};

export function RequestStatus({ status, state, assessment }: RequestStatusProps) {
  const presentation = requestStatusPresentation({ status, state, assessment });

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
    <span className={styles.compactIssue}>
      <IssueIndicator
        tone={issue.tone}
        label={issue.label}
        message={issue.message}
        ariaLabel={assessmentIssueText(issue)}
      />
    </span>
  );
}

interface RecordHeadlineStatusProps {
  response: ResponseMetadata | null;
  state: RequestState;
  assessment: RequestAssessment;
}

export function RecordHeadlineStatus({ response, state, assessment }: RecordHeadlineStatusProps) {
  const presentation = requestHeadlinePresentation(response, state, assessment);
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
