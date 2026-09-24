import { Check, ChevronDown, Clipboard } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { imageTitle, formatDuration } from "@/features/overview/components/runtimeImage";
import type { OverviewData } from "@/api/overview";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { IconButton } from "@/shared/ui/IconButton";
import { storePreference } from "@/shared/lib/preferences";
import styles from "@/features/overview/OverviewPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const OPEN_KEY = "aibox-console-environment-details-open";

export function EnvironmentDetails({
  overview,
  elapsedUptime,
}: {
  overview: OverviewData | null;
  elapsedUptime: number;
}) {
  const [copied, copy] = useClipboardFeedback<string>();
  const [open, setOpen] = useState(false);
  const detailsRef = useRef<HTMLDetailsElement>(null);

  // Clear any legacy persistent open state so it never defaults to open on reload
  useEffect(() => {
    storePreference(OPEN_KEY, "false");
  }, []);

  useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: PointerEvent) {
      if (detailsRef.current && !detailsRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  return (
    <details
      ref={detailsRef}
      className={styles.environmentDetails}
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary className={styles.environmentSummary}>
        Environment details
        <ChevronDown
          size={iconSize.xs}
          className={`${styles.envChevron} ${open ? styles.envChevronOpen : ""}`}
          aria-hidden="true"
        />
      </summary>
      <dl>
        {[
          ["Listen", overview?.service.listen],
          ["AIBox Root", overview?.service.aibox_root],
        ].map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>
              <code>{value ?? "Loading"}</code>
              <IconButton
                size="sm"
                label={`Copy ${label}`}
                disabled={!value}
                onClick={() => value && void copy(value, label!)}
              >
                {copied === label ? <Check size={iconSize.xs} /> : <Clipboard size={iconSize.xs} />}
              </IconButton>
            </dd>
          </div>
        ))}
        <div>
          <dt>Uptime</dt>
          <dd>{overview ? formatDuration(elapsedUptime) : "Loading"}</dd>
        </div>
        <div>
          <dt>Runtime Image</dt>
          <dd>
            <code>{overview ? imageTitle(overview.runtime_image) : "Loading"}</code>
          </dd>
        </div>
      </dl>
      <span className="srOnly" role="status">
        {copied ? `${copied} copied` : ""}
      </span>
    </details>
  );
}
