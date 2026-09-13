import { ChevronDown, ChevronRight, ChevronUp, LoaderCircle } from "lucide-react";
import { useState } from "react";

import type { AttentionItem } from "@/features/overview/resourceTree";
import { OverviewLink } from "@/features/overview/OverviewLink";
import type { AttentionPanelKind } from "@/features/overview/viewTypes";
import { toneIcons } from "@/shared/icons/consoleIcons";
import { iconSize } from "@/shared/icons/iconSizes";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/overview/components/AttentionPanel.module.css";

/**
 * How many conditions the panel shows before it asks.
 *
 * A healthy Service reports none and a normal unhealthy one reports a handful;
 * the cap exists for the pathological case — a Tenant whose Component catalog
 * is unreadable contributes a row per Tenant — where an uncapped list would
 * push the Tenant table off the screen the panel is supposed to be triaging.
 */
const VISIBLE_LIMIT = 6;

interface AttentionPanelProps {
  panel: AttentionPanelKind;
  items: AttentionItem[];
  onNavigate: ConsoleNavigate;
  onRetry: (source: "service" | "topology") => void;
  serviceRetrying: boolean;
  topologyRetrying: boolean;
}

export function AttentionPanel({
  panel,
  items,
  onNavigate,
  onRetry,
  serviceRetrying,
  topologyRetrying,
}: AttentionPanelProps) {
  const [showAll, setShowAll] = useState(false);
  const overflow = Math.max(0, items.length - VISIBLE_LIMIT);
  /*
   * A shrinking list needs no reset: under the cap the slice already yields
   * every item and the toggle hides itself, so a stale expansion is invisible
   * until the list is long enough for it to mean something again.
   */
  const visible = showAll ? items : items.slice(0, VISIBLE_LIMIT);

  return (
    <section className={styles.panel} aria-labelledby="attention-title">
      <h2 id="attention-title" className="srOnly">
        Needs attention
      </h2>
      {panel === "pending" ? (
        <p className={styles.quiet} role="status">
          <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
          Inspecting service and topology
        </p>
      ) : panel === "healthy" ? (
        <p className={styles.quiet} role="status">
          No warnings or errors are currently reported.
        </p>
      ) : (
        <>
          <p className={styles.count} role="status">
            Needs attention · {items.length}
          </p>
          <ul className={styles.list}>
            {visible.map((item) => (
              <AttentionRow
                key={item.key}
                item={item}
                onNavigate={onNavigate}
                onRetry={onRetry}
                retrying={item.retry === "service" ? serviceRetrying : topologyRetrying}
              />
            ))}
          </ul>
          {overflow > 0 && (
            <button
              type="button"
              className={styles.moreToggle}
              aria-expanded={showAll}
              onClick={() => setShowAll((current) => !current)}
            >
              {showAll ? (
                <ChevronUp size={iconSize.xs} aria-hidden="true" />
              ) : (
                <ChevronDown size={iconSize.xs} aria-hidden="true" />
              )}
              {showAll ? "Show fewer" : `Show ${overflow} more`}
            </button>
          )}
        </>
      )}
    </section>
  );
}

interface AttentionRowProps {
  item: AttentionItem;
  onNavigate: ConsoleNavigate;
  onRetry: (source: "service" | "topology") => void;
  retrying: boolean;
}

function AttentionRow({ item, onNavigate, onRetry, retrying }: AttentionRowProps) {
  const ToneIcon = toneIcons[item.tone];
  const content = (
    <>
      <span className={styles.mark}>
        <ToneIcon size={iconSize.xs} aria-hidden="true" />
      </span>
      <span className={styles.label}>{item.label}</span>
      <span className={styles.detail}>{item.detail}</span>
    </>
  );
  /*
   * The raw diagnostic is evidence, not the message. It sits in the detail
   * column so it opens under the sentence it belongs to rather than at the
   * panel's right edge, and it stays closed until asked for.
   */
  const technical = item.technical && (
    <details className={styles.technical}>
      <summary>Technical details</summary>
      <p>{item.technical}</p>
    </details>
  );
  return (
    <li className={`${styles.row} ${styles[item.tone]}`}>
      {item.target ? (
        <OverviewLink
          className={styles.rowLink}
          targetModule={item.target.module}
          query={item.target.query}
          onNavigate={onNavigate}
        >
          {content}
          <ChevronRight className={styles.chevron} size={iconSize.xs} aria-hidden="true" />
        </OverviewLink>
      ) : (
        <div className={styles.rowStatic}>
          {content}
          {technical}
          {item.retry && (
            <RefreshButton
              className={styles.retry}
              tone="secondary"
              label={`Retry ${item.retry === "service" ? "Service status" : "resource inspection"}`}
              busy={retrying}
              disabled={retrying}
              onClick={() => onRetry(item.retry === "service" ? "service" : "topology")}
            />
          )}
        </div>
      )}
    </li>
  );
}
