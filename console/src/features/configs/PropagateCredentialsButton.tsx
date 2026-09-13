import { KeyRound } from "lucide-react";
import { ActionButton } from "@/shared/ui/ActionButton";
import { iconSize } from "@/shared/icons/iconSizes";

/**
 * The one action that rewrites files outside the selected Config, offered
 * from the Current Config row and its detail header. It stays focusable while
 * the preview loads so the dialog can hand focus back to it on Close.
 */
export function PropagateCredentialsButton({
  busy,
  compact = false,
  className,
  onClick,
}: {
  busy: boolean;
  /** The row keeps the verb; the header carries the full name. */
  compact?: boolean;
  className?: string;
  onClick: () => void;
}) {
  return (
    <ActionButton
      tone="secondary"
      className={className}
      aria-label={compact ? "Propagate credentials" : undefined}
      aria-disabled={busy || undefined}
      aria-busy={busy || undefined}
      onClick={busy ? undefined : onClick}
    >
      <KeyRound size={iconSize.xs} aria-hidden="true" />
      {compact ? "Propagate" : "Propagate credentials"}
    </ActionButton>
  );
}
