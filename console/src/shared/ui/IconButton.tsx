import type { ComponentProps, ReactNode, Ref, RefObject } from "react";
import { forwardRef } from "react";
import { ActionButton, type ActionButtonTone } from "@/shared/ui/ActionButton";
import { AnchoredTooltip } from "@/shared/ui/AnchoredTooltip";
import styles from "@/shared/ui/IconButton.module.css";

/*
 * Long enough that sweeping a pointer across a row of actions stays quiet,
 * short enough to answer someone who stopped to ask. Focus does not wait,
 * because a keyboard user has no other way to read the icon.
 */
const HOVER_DELAY_MS = 450;

/**
 * Which step the control takes. `md` is a control in its own slot: a row or
 * toolbar action. `sm` rides inside a line of text beside the value it acts on,
 * and still meets the touch-target floor on a coarse pointer.
 */
export type IconButtonSize = "md" | "sm";

type IconButtonProps = Omit<ComponentProps<typeof ActionButton>, "children"> & {
  label: string;
  children: ReactNode;
  /** @deprecated Prefer the forwarded ref. Kept for existing call sites. */
  buttonRef?: RefObject<HTMLButtonElement | null>;
  tone?: Extract<ActionButtonTone, "ghost" | "dangerQuiet" | "danger">;
  size?: IconButtonSize;
};

/**
 * A button whose only content is an icon, and which therefore owes the reader
 * its name. The label is the accessible name and the tooltip text both: a
 * trash icon in a list of fifty rows says nothing about which row it ends, and
 * that answer already exists here.
 */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, children, className, buttonRef, tone = "ghost", size = "md", ...props },
  ref,
) {
  return (
    <AnchoredTooltip<HTMLButtonElement>
      openDelayMs={HOVER_DELAY_MS}
      disabled={Boolean(props.disabled)}
      content={label}
      className={styles.tooltip}
      positionKey={label}
    >
      {(tooltip) => {
        const {
          onPointerEnter,
          onPointerLeave,
          onPointerDown,
          onFocus,
          onBlur,
          onKeyDown,
          onClick,
          ...buttonProps
        } = props;
        return (
          <ActionButton
            {...buttonProps}
            ref={(element) => {
              tooltip.triggerRef.current = element;
              if (buttonRef) buttonRef.current = element;
              assignRef(ref, element);
            }}
            className={`${styles.button} ${size === "sm" ? styles.sm : ""} ${className ?? ""}`}
            data-icon-button="true"
            data-icon-button-size={size}
            type="button"
            tone={tone}
            aria-label={label}
            aria-describedby={tooltip.describedBy}
            onPointerEnter={(event) => {
              onPointerEnter?.(event);
              tooltip.onPointerEnter(event);
            }}
            onPointerLeave={(event) => {
              onPointerLeave?.(event);
              tooltip.onPointerLeave(event);
            }}
            onPointerDown={(event) => {
              onPointerDown?.(event);
              tooltip.onPointerDown(event);
            }}
            onFocus={(event) => {
              onFocus?.(event);
              tooltip.onFocus(event);
            }}
            onBlur={(event) => {
              onBlur?.(event);
              tooltip.onBlur(event);
            }}
            onKeyDown={(event) => {
              onKeyDown?.(event);
              if (!event.defaultPrevented) tooltip.onKeyDown(event);
            }}
            onClick={(event) => {
              onClick?.(event);
              tooltip.close();
            }}
          >
            {children}
          </ActionButton>
        );
      }}
    </AnchoredTooltip>
  );
});

function assignRef(ref: Ref<HTMLButtonElement>, element: HTMLButtonElement | null): void {
  if (typeof ref === "function") ref(element);
  else if (ref) (ref as { current: HTMLButtonElement | null }).current = element;
}
