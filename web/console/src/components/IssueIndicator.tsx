import { CircleAlert, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";
import styles from "./IssueIndicator.module.css";

const TOOLTIP_DELAY_MS = 150;
const TOOLTIP_GAP_PX = 8;
const VIEWPORT_MARGIN_PX = 8;
const TOOLTIP_MAX_WIDTH_PX = 320;

export type IssueTone = "error" | "warning";

interface TooltipPosition {
  left: number;
  top: number;
}

export interface IssueTooltipProps {
  tone: IssueTone;
  label: string;
  message: string;
  className: string;
  children: ReactNode;
  role?: "img";
  ariaLabel?: string;
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
      role="img"
      ariaLabel={ariaLabel}
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
  role,
  ariaLabel,
}: IssueTooltipProps) {
  const triggerRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const openTimer = useRef<number | null>(null);
  const tooltipId = useId();
  const [pending, setPending] = useState(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<TooltipPosition | null>(null);
  const toneLabel = tone === "error" ? "Error" : "Warning";

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
  }, [label, message, open]);

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
              <span className={styles[tone]}>{toneLabel}</span>
              <span aria-hidden="true"> · </span>
              {label}
            </div>
            <div className={styles.tooltipMessage}>{message}</div>
          </div>,
          document.body,
        )}
    </>
  );
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), maximum);
}
