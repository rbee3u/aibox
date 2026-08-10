import { CircleAlert, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";
import type { RecordAssessment, RecordState, ResponseMetadata } from "../types";
import styles from "./RecordStatus.module.css";
import {
  assessmentIssueText,
  recordHeadlinePresentation,
  recordStatusPresentation,
  type AssessmentPresentation,
  type RecordStatusTone,
} from "./statusPresentation";

const TOOLTIP_DELAY_MS = 150;
const TOOLTIP_GAP_PX = 8;
const VIEWPORT_MARGIN_PX = 8;
const TOOLTIP_MAX_WIDTH_PX = 320;

interface TooltipPosition {
  left: number;
  top: number;
}

interface RecordStatusProps {
  status: number | null;
  state: RecordState;
  assessment: RecordAssessment;
  compact?: boolean;
}

const TONE_CLASS: Record<RecordStatusTone, string> = {
  active: styles.active,
  error: styles.error,
  neutral: styles.neutral,
  success: styles.success,
  warning: styles.warningTone,
};

export function RecordStatus({ status, state, assessment, compact = false }: RecordStatusProps) {
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
      {presentation.issue && compact && <CompactIssue issue={presentation.issue} />}
    </span>
  );
}

function CompactIssue({ issue }: { issue: AssessmentPresentation }) {
  const IssueIcon = issue.tone === "error" ? CircleAlert : TriangleAlert;
  return (
    <IssueTooltip
      issue={issue}
      className={`${styles.issue} ${TONE_CLASS[issue.tone]}`}
      role="img"
      ariaLabel={assessmentIssueText(issue)}
    >
      <IssueIcon size={13} strokeWidth={2.2} aria-hidden="true" />
    </IssueTooltip>
  );
}

interface IssueTooltipProps {
  issue: AssessmentPresentation;
  className: string;
  children: ReactNode;
  role?: "img";
  ariaLabel?: string;
}

function IssueTooltip({ issue, className, children, role, ariaLabel }: IssueTooltipProps) {
  const triggerRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const openTimer = useRef<number | null>(null);
  const tooltipId = useId();
  const [pending, setPending] = useState(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<TooltipPosition | null>(null);
  const toneLabel = issue.tone === "error" ? "Error" : "Warning";

  const clearOpenTimer = useCallback(() => {
    if (openTimer.current === null) return;
    window.clearTimeout(openTimer.current);
    openTimer.current = null;
  }, []);

  const close = useCallback(() => {
    clearOpenTimer();
    setPending(false);
    setOpen(false);
    setPosition(null);
  }, [clearOpenTimer]);

  function scheduleOpen() {
    clearOpenTimer();
    setPending(true);
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      setPending(false);
      setOpen(true);
    }, TOOLTIP_DELAY_MS);
  }

  useEffect(() => clearOpenTimer, [clearOpenTimer]);

  useEffect(() => {
    if (!pending && !open) return;
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [close, open, pending]);

  useLayoutEffect(() => {
    if (!open || !triggerRef.current || !tooltipRef.current) return;
    const triggerRect = triggerRef.current.getBoundingClientRect();
    const tooltipRect = tooltipRef.current.getBoundingClientRect();
    const viewportWidth = document.documentElement.clientWidth || window.innerWidth;
    const viewportHeight = document.documentElement.clientHeight || window.innerHeight;
    const tooltipWidth = Math.min(tooltipRect.width, TOOLTIP_MAX_WIDTH_PX);
    const leftSpace = triggerRect.left - TOOLTIP_GAP_PX - VIEWPORT_MARGIN_PX;
    const preferredLeft =
      leftSpace >= tooltipWidth
        ? triggerRect.left - TOOLTIP_GAP_PX - tooltipWidth
        : triggerRect.right + TOOLTIP_GAP_PX;
    const left = clamp(
      preferredLeft,
      VIEWPORT_MARGIN_PX,
      viewportWidth - tooltipWidth - VIEWPORT_MARGIN_PX,
    );
    const top = clamp(
      triggerRect.top + triggerRect.height / 2 - tooltipRect.height / 2,
      VIEWPORT_MARGIN_PX,
      viewportHeight - tooltipRect.height - VIEWPORT_MARGIN_PX,
    );
    setPosition({ left, top });
  }, [issue.label, issue.message, open]);

  return (
    <>
      <span
        ref={triggerRef}
        className={className}
        role={role}
        aria-label={ariaLabel}
        aria-describedby={open ? tooltipId : undefined}
        onPointerEnter={scheduleOpen}
        onPointerLeave={close}
      >
        {children}
      </span>
      {open &&
        createPortal(
          <div
            ref={tooltipRef}
            id={tooltipId}
            className={`${styles.tooltip} ${position ? styles.tooltipVisible : ""}`}
            role="tooltip"
            style={position ?? undefined}
          >
            <div className={styles.tooltipTitle}>
              <span className={TONE_CLASS[issue.tone]}>{toneLabel}</span>
              <span aria-hidden="true"> · </span>
              {issue.label}
            </div>
            <div className={styles.tooltipMessage}>{issue.message}</div>
          </div>,
          document.body,
        )}
    </>
  );
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
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
          issue={presentation.tag}
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
