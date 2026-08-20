import { CircleAlert, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ReactNode, RefObject } from "react";
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
  ariaLabel?: string;
  interactive?: boolean;
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
  const triggerRef = useRef<HTMLElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const openTimer = useRef<number | null>(null);
  const tooltipId = useId();
  const descriptionId = useId();
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

  const openImmediately = useCallback(() => {
    clearOpenTimer();
    setPending(false);
    setOpen(true);
  }, [clearOpenTimer]);

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

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close();
      triggerRef.current?.focus();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [close, open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerRef.current?.contains(target) || tooltipRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [close, open]);

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
      {interactive ? (
        <button
          type="button"
          ref={triggerRef as RefObject<HTMLButtonElement>}
          className={className}
          aria-label={ariaLabel ?? `${toneLabel}: ${label}`}
          aria-describedby={open ? tooltipId : descriptionId}
          aria-expanded={open}
          onPointerEnter={scheduleOpen}
          onPointerLeave={() => {
            if (document.activeElement !== triggerRef.current) close();
          }}
          onFocus={openImmediately}
          onBlur={close}
          onClick={openImmediately}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            close();
            event.currentTarget.focus();
          }}
        >
          {children}
        </button>
      ) : (
        <span
          ref={triggerRef}
          className={className}
          role="img"
          aria-label={ariaLabel ?? `${toneLabel}: ${label}`}
          aria-describedby={open ? tooltipId : descriptionId}
          onPointerEnter={scheduleOpen}
          onPointerLeave={(event) => {
            if (event.pointerType !== "touch") close();
          }}
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            openImmediately();
          }}
        >
          {children}
        </span>
      )}
      <span id={descriptionId} className="srOnly">
        {toneLabel}: {label}. {message}
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
