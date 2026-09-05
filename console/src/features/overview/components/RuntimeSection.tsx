import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, ChevronDown, Hammer, LoaderCircle, RefreshCw } from "lucide-react";
import type { Operation } from "@/api/operations";
import type { OverviewData } from "@/api/overview";
import { buildActionTone, cachelessBuildInline } from "@/features/overview/components/runtimeImage";
import { ActionButton } from "@/shared/ui/ActionButton";
import styles from "@/features/overview/OverviewPage.module.css";

interface RuntimeSectionProps {
  overview: OverviewData | null;
  /** The running Operation, when one owns the Runtime Image. */
  operation: Operation | null;
  buildDisabled: boolean;
  /** Explains a disabled build, and is announced through aria-describedby. */
  buildUnavailableReason: string | null;
  onBuild: (force: boolean) => void;
}

/**
 * Overview is the only Runtime Image build entry point. The status strip owns
 * Docker and image state; this cluster only offers the two build actions.
 */
export function RuntimeSection({
  overview,
  operation,
  buildDisabled,
  buildUnavailableReason,
  onBuild,
}: RuntimeSectionProps) {
  const operationRunning = operation?.state === "running";
  const status = overview?.runtime_image.status;
  const inlineCacheless = cachelessBuildInline(status);
  const describedBy = buildUnavailableReason ? "runtime-build-unavailable" : undefined;
  return (
    <>
      {operationRunning && operation && (
        <span className={styles.operationState} title={operation.kind}>
          <LoaderCircle className="spin" size={14} /> {operation.kind}
        </span>
      )}
      <div className={inlineCacheless ? styles.buildActions : styles.buildSplit}>
        <ActionButton
          tone={buildActionTone(status)}
          className={inlineCacheless ? undefined : styles.buildSplitPrimary}
          disabled={buildDisabled}
          aria-describedby={describedBy}
          onClick={() => onBuild(false)}
        >
          <Hammer size={15} aria-hidden="true" /> Build
        </ActionButton>
        {inlineCacheless ? (
          <CachelessBuildButton
            disabled={buildDisabled}
            describedBy={describedBy}
            onBuild={onBuild}
          />
        ) : (
          <BuildOverflow disabled={buildDisabled} describedBy={describedBy} onBuild={onBuild} />
        )}
      </div>
    </>
  );
}

function CachelessBuildButton({
  disabled,
  describedBy,
  onBuild,
}: {
  disabled: boolean;
  describedBy?: string;
  onBuild: (force: boolean) => void;
}) {
  return (
    <ActionButton disabled={disabled} aria-describedby={describedBy} onClick={() => onBuild(true)}>
      <RefreshCw size={15} aria-hidden="true" /> Build without cache
    </ActionButton>
  );
}

function BuildOverflow({
  disabled,
  describedBy,
  onBuild,
}: {
  disabled: boolean;
  describedBy?: string;
  onBuild: (force: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; left: number } | undefined>();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const itemRef = useRef<HTMLButtonElement>(null);

  useLayoutEffect(() => {
    if (!open) return;
    function placeMenu() {
      const trigger = triggerRef.current;
      const menu = menuRef.current;
      if (!trigger || !menu) return;
      const gap = 6;
      const margin = 8;
      const triggerRect = trigger.getBoundingClientRect();
      const menuRect = menu.getBoundingClientRect();
      const left = Math.min(
        Math.max(margin, triggerRect.right - menuRect.width),
        Math.max(margin, window.innerWidth - menuRect.width - margin),
      );
      const below = triggerRect.bottom + gap;
      const top =
        below + menuRect.height <= window.innerHeight - margin
          ? below
          : Math.max(margin, triggerRect.top - menuRect.height - gap);
      setPosition({ left, top });
    }
    placeMenu();
    window.addEventListener("resize", placeMenu);
    window.addEventListener("scroll", placeMenu, true);
    return () => {
      window.removeEventListener("resize", placeMenu);
      window.removeEventListener("scroll", placeMenu, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    itemRef.current?.focus();
    function closeOnPointerDown(event: PointerEvent) {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      setOpen(false);
    }
    function closeOnKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
    }
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnKeyDown);
    return () => {
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnKeyDown);
    };
  }, [open]);

  return (
    <>
      <ActionButton
        ref={triggerRef}
        className={styles.buildSplitTrigger}
        disabled={disabled}
        aria-label="More build options"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? "runtime-build-menu" : undefined}
        aria-describedby={describedBy}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          setOpen(true);
        }}
      >
        <ChevronDown size={14} aria-hidden="true" />
      </ActionButton>
      {open &&
        createPortal(
          <div
            id="runtime-build-menu"
            ref={menuRef}
            className={styles.buildMenu}
            role="menu"
            aria-label="Build options"
            style={position ?? { visibility: "hidden" }}
          >
            <button
              ref={itemRef}
              type="button"
              role="menuitem"
              disabled={disabled}
              aria-describedby={describedBy}
              onKeyDown={(event) => {
                if (event.key === "Tab") setOpen(false);
              }}
              onClick={() => {
                setOpen(false);
                onBuild(true);
              }}
            >
              <RefreshCw size={15} aria-hidden="true" /> Build without cache
            </button>
          </div>,
          document.body,
        )}
    </>
  );
}

export function RuntimeNotices({
  overview,
  buildUnavailableReason,
}: {
  overview: OverviewData | null;
  buildUnavailableReason: string | null;
}) {
  return (
    <>
      {buildUnavailableReason && (
        <p id="runtime-build-unavailable" className={styles.buildUnavailable} role="status">
          {buildUnavailableReason}
        </p>
      )}
      {overview?.runtime_image.detail && (
        <div className={styles.runtimeNotice} role="status">
          <AlertTriangle size={15} /> {overview.runtime_image.detail}
        </div>
      )}
    </>
  );
}
