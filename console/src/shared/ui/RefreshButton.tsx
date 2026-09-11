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

export const RefreshButton = forwardRef<HTMLButtonElement, RefreshButtonProps>(
  function RefreshButton(
    {
      label,
      busy = false,
      busyLabel,
      children,
      className,
      compactOnNarrow = false,
      iconSize = 14,
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
      >
        {children ?? label}
      </IconLabelButton>
    );
  },
);
