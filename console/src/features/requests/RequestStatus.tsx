import type { RequestAssessment, RequestState, ResponseMetadata } from "@/api/requests";
import styles from "@/features/requests/RequestStatus.module.css";
import { IssueTooltip } from "@/shared/ui/IssueIndicator";
import { StatusBadge, type StatusTone } from "@/shared/ui/StatusBadge";
import {
  assessmentCatalogLabel,
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

const STATUS_BADGE_TONE: Record<RequestStatusTone, StatusTone> = {
  active: "active",
  error: "error",
  neutral: "neutral",
  success: "good",
  warning: "warning",
};

export function RequestStatus({ status, state, assessment }: RequestStatusProps) {
  const presentation = requestStatusPresentation({ status, state, assessment });

  return (
    <span className={styles.root}>
      <StatusBadge tone={STATUS_BADGE_TONE[presentation.tone]} variant="inline">
        {presentation.label}
      </StatusBadge>
      {presentation.phase && (
        <StatusBadge tone="active" variant="inline">
          {presentation.phase}
        </StatusBadge>
      )}
    </span>
  );
}

export function RequestCatalogIssue({ issue }: { issue: AssessmentPresentation }) {
  return (
    <IssueTooltip
      tone={issue.tone}
      label={issue.label}
      message={issue.message}
      className={`${styles.catalogIssue} ${
        issue.tone === "warning" ? styles.catalogWarning : styles.catalogError
      }`}
      ariaLabel={assessmentIssueText(issue)}
      interactive={false}
    >
      <span className={styles.catalogIssueText}>{assessmentCatalogLabel(issue)}</span>
    </IssueTooltip>
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
          <span className={styles.dot} aria-hidden="true" />
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
