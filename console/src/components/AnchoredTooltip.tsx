import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  FocusEventHandler,
  KeyboardEventHandler,
  PointerEventHandler,
  ReactNode,
  RefObject,
} from "react";
import styles from "./AnchoredTooltip.module.css";

const TOOLTIP_GAP_PX = 8;
const VIEWPORT_MARGIN_PX = 8;
const TOOLTIP_MAX_WIDTH_PX = 320;

interface TooltipPosition {
  left: number;
  top: number;
}

export interface AnchoredTooltipBindings<T extends HTMLElement> {
  triggerRef: RefObject<T | null>;
  describedBy: string | undefined;
  open: boolean;
  onPointerEnter: PointerEventHandler<T>;
  onPointerLeave: PointerEventHandler<T>;
  onPointerDown: PointerEventHandler<T>;
  onFocus: FocusEventHandler<T>;
  onBlur: FocusEventHandler<T>;
  onKeyDown: KeyboardEventHandler<T>;
  openImmediately: () => void;
  close: () => void;
}

export function AnchoredTooltip<T extends HTMLElement>({
  openDelayMs,
  accessibleDescription,
  disabled = false,
  content,
  className,
  positionKey,
  children,
}: {
  openDelayMs: number;
  accessibleDescription?: string;
  disabled?: boolean;
  content: ReactNode;
  className: string;
  positionKey?: string;
  children: (bindings: AnchoredTooltipBindings<T>) => ReactNode;
}) {
  const triggerRef = useRef<T>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const openTimer = useRef<number | null>(null);
  const tooltipId = useId();
  const descriptionId = useId();
  const [pending, setPending] = useState(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<TooltipPosition | null>(null);

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

  const scheduleOpen = useCallback(() => {
    clearOpenTimer();
    setPending(true);
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      setPending(false);
      setOpen(true);
    }, openDelayMs);
  }, [clearOpenTimer, openDelayMs]);

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

  const visible = open && !disabled;

  useEffect(() => {
    if (!visible) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close();
      triggerRef.current?.focus();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [close, visible]);

  useEffect(() => {
    if (!visible) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerRef.current?.contains(target) || tooltipRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [close, visible]);

  useLayoutEffect(() => {
    if (!visible || !triggerRef.current || !tooltipRef.current) return;
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
  }, [positionKey, visible]);

  const bindings: AnchoredTooltipBindings<T> = {
    triggerRef,
    describedBy: visible ? tooltipId : accessibleDescription ? descriptionId : undefined,
    open: visible,
    onPointerEnter: () => {
      if (!disabled) scheduleOpen();
    },
    onPointerLeave: (event) => {
      if (event.pointerType !== "touch" && document.activeElement !== triggerRef.current) close();
    },
    onPointerDown: (event) => {
      if (!disabled && event.pointerType !== "mouse") openImmediately();
    },
    onFocus: () => {
      if (!disabled) openImmediately();
    },
    onBlur: () => close(),
    onKeyDown: (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close();
      event.currentTarget.focus();
    },
    openImmediately,
    close,
  };

  return (
    <>
      {children(bindings)}
      {accessibleDescription && (
        <span id={descriptionId} className="srOnly">
          {accessibleDescription}
        </span>
      )}
      {visible &&
        createPortal(
          <div
            ref={tooltipRef}
            id={tooltipId}
            className={`${styles.surface} ${position ? styles.visible : ""} ${className}`}
            role="tooltip"
            style={position ?? undefined}
          >
            {content}
          </div>,
          document.body,
        )}
    </>
  );
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), maximum);
}
