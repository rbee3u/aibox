import { CircleAlert, CircleHelp, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { AnchoredTooltip } from "./AnchoredTooltip";
import styles from "./IssueIndicator.module.css";

const TOOLTIP_DELAY_MS = 150;

export type IssueTone = "error" | "warning";
type TooltipTone = IssueTone | "help";

export interface IssueTooltipProps {
  tone: TooltipTone;
  label: string;
  message: string;
  className: string;
  children: ReactNode;
  ariaLabel?: string;
  interactive?: boolean;
}

export function HelpTooltip({ label, message }: { label: string; message: string }) {
  return (
    <IssueTooltip
      tone="help"
      label={label}
      message={message}
      className={`${styles.indicator} ${styles.help}`}
      ariaLabel={`Help for ${label}`}
    >
      <CircleHelp size={14} strokeWidth={2} aria-hidden="true" />
    </IssueTooltip>
  );
}

export function IssueIndicator({
  tone,
  label,
  message,
  ariaLabel,
}: {
  tone: IssueTone;
  label: string;
  message: string;
  ariaLabel: string;
}) {
  const IssueIcon = tone === "error" ? CircleAlert : TriangleAlert;
  return (
    <IssueTooltip
      tone={tone}
      label={label}
      message={message}
      className={`${styles.indicator} ${styles[tone]}`}
      ariaLabel={ariaLabel}
      interactive={false}
    >
      <IssueIcon size={13} strokeWidth={2.2} aria-hidden="true" />
    </IssueTooltip>
  );
}

export function IssueTooltip({
  tone,
  label,
  message,
  className,
  children,
  ariaLabel,
  interactive = true,
}: IssueTooltipProps) {
  const toneLabel = tone === "error" ? "Error" : tone === "warning" ? "Warning" : "Help";

  return (
    <AnchoredTooltip<HTMLElement>
      openDelayMs={TOOLTIP_DELAY_MS}
      accessibleDescription={`${toneLabel}: ${label}. ${message}`}
      className={styles.tooltip}
      positionKey={`${label}:${message}`}
      content={
        <>
          <div className={styles.tooltipTitle}>
            <span className={styles[tone]}>{toneLabel}</span>
            <span aria-hidden="true"> · </span>
            {label}
          </div>
          <div className={styles.tooltipMessage}>{message}</div>
        </>
      }
    >
      {(tooltip) =>
        interactive ? (
          <button
            type="button"
            ref={tooltip.triggerRef as React.RefObject<HTMLButtonElement | null>}
            className={className}
            aria-label={ariaLabel ?? `${toneLabel}: ${label}`}
            aria-describedby={tooltip.describedBy}
            aria-expanded={tooltip.open}
            onPointerEnter={tooltip.onPointerEnter}
            onPointerLeave={tooltip.onPointerLeave}
            onPointerDown={tooltip.onPointerDown}
            onFocus={tooltip.onFocus}
            onBlur={tooltip.onBlur}
            onClick={tooltip.openImmediately}
            onKeyDown={tooltip.onKeyDown}
          >
            {children}
          </button>
        ) : (
          <span
            ref={tooltip.triggerRef}
            className={className}
            role="img"
            aria-label={ariaLabel ?? `${toneLabel}: ${label}`}
            aria-describedby={tooltip.describedBy}
            onPointerEnter={tooltip.onPointerEnter}
            onPointerLeave={tooltip.onPointerLeave}
            onPointerDown={tooltip.onPointerDown}
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              tooltip.openImmediately();
            }}
          >
            {children}
          </span>
        )
      }
    </AnchoredTooltip>
  );
}
