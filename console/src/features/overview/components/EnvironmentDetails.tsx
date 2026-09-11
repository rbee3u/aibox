import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import { imageTitle, formatDuration } from "@/features/overview/components/runtimeImage";
import type { OverviewData } from "@/api/overview";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { IconButton } from "@/shared/ui/IconButton";
import { readPreference, storePreference } from "@/shared/lib/preferences";
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
  /*
   * Listen and AIBox Root answer which instance is on screen, so someone
   * running more than one should not have to reopen this after every reload
   * and every module change. The sidebar and the theme already persist.
   */
  const [open, setOpen] = useState(() => readPreference(OPEN_KEY) === "true");
  useEffect(() => {
    storePreference(OPEN_KEY, String(open));
  }, [open]);
  return (
    <details
      className={styles.environmentDetails}
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>Environment details</summary>
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
                {copied === label ? <Check size={iconSize.xs} /> : <Copy size={iconSize.xs} />}
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
