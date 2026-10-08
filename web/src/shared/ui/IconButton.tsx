import type { ComponentProps, ReactNode, Ref, RefObject } from "react";
import { forwardRef } from "react";
import { ActionButton, type ActionButtonTone } from "@/shared/ui/ActionButton";
import { AnchoredTooltip } from "@/shared/ui/AnchoredTooltip";
import styles from "@/shared/ui/IconButton.module.css";

const HOVER_DELAY_MS = 450;

/**
 * `sm` fits inline text; `md` fits row and toolbar actions. Both retain the
 * coarse-pointer target minimum.
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
