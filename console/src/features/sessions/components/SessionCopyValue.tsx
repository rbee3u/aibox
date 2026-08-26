import { Check, Clipboard } from "lucide-react";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { IconButton } from "@/shared/ui/IconButton";
import styles from "@/features/sessions/SessionPage.module.css";

/** Technical fact with an inline copy control, such as the Session ID. */
export function SessionCopyValue({ label, value }: { label: string; value: string }) {
  const [copied, copy] = useClipboardFeedback();
  return (
    <span className={styles.sessionCopyValue}>
      <code>{value}</code>
      <IconButton
        className={styles.sessionCopyAction}
        label={copied ? `${label} copied` : `Copy ${label}`}
        onClick={() => void copy(value, true)}
      >
        {copied ? (
          <Check size={13} aria-hidden="true" />
        ) : (
          <Clipboard size={13} aria-hidden="true" />
        )}
      </IconButton>
    </span>
  );
}
