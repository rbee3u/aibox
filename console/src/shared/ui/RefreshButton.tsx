import { RefreshCw } from "lucide-react";
import { forwardRef, type ReactNode } from "react";
import { type ActionButtonProps, type ActionButtonTone } from "@/shared/ui/ActionButton";
import { IconLabelButton } from "@/shared/ui/IconLabelButton";

export interface RefreshButtonProps extends Omit<
  ActionButtonProps,
  "aria-label" | "children" | "title"
> {
  label: string;
  busy?: boolean;
  busyLabel?: string;
  children?: ReactNode;
  compactOnNarrow?: boolean;
  iconSize?: number;
  tone?: Extract<ActionButtonTone, "ghost" | "secondary">;
}

/**
 * A `busy` Refresh stays focusable. Disabling the button the user just
 * activated makes the browser drop focus to `<body>`, so the reload the user
 * asked for would end with no idea where they are; the spinning icon and
 * `aria-disabled` already say it cannot be pressed again. `disabled` still
 * applies for the caller's other reasons once the reload is over.
 */
export const RefreshButton = forwardRef<HTMLButtonElement, RefreshButtonProps>(
  function RefreshButton(
    {
      label,
      busy = false,
      busyLabel,
      children,
      className,
      compactOnNarrow = false,
      disabled = false,
      iconSize = 14,
      onClick,
      tone = "ghost",
      ...props
    },
    ref,
  ) {
    return (
      <IconLabelButton
        {...props}
        ref={ref}
        className={className}
        compactOnNarrow={compactOnNarrow}
        icon={
          <RefreshCw className={busy ? "spin" : undefined} size={iconSize} aria-hidden="true" />
        }
        data-refresh-button="true"
        tone={tone}
        aria-label={busy && busyLabel ? busyLabel : label}
        aria-busy={busy || undefined}
        aria-disabled={busy || undefined}
        disabled={!busy && disabled}
        onClick={busy ? undefined : onClick}
      >
        {children ?? label}
      </IconLabelButton>
    );
  },
);
