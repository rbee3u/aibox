import type { ComponentProps, ReactNode, RefObject } from "react";
import { ActionButton } from "./ActionButton";
import { AnchoredTooltip } from "./AnchoredTooltip";
import styles from "./IconButton.module.css";

export function IconButton({
  label,
  children,
  className,
  buttonRef,
  ...props
}: Omit<ComponentProps<typeof ActionButton>, "children" | "tone"> & {
  label: string;
  children: ReactNode;
  buttonRef?: RefObject<HTMLButtonElement | null>;
}) {
  return (
    <AnchoredTooltip<HTMLButtonElement>
      openDelayMs={450}
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
            }}
            className={`${styles.button} ${className ?? ""}`}
            data-icon-button="true"
            type="button"
            tone="quiet"
            title={label}
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
}
