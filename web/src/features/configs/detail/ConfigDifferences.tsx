import { AlertTriangle, Check, LoaderCircle } from "lucide-react";
import { useState } from "react";
import type { ConfigDifference } from "@/api/configs";
import { useConfigComparison } from "@/features/configs/detail/ConfigComparisonContext";
import styles from "@/features/configs/ConfigPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

export function differenceId(file: string, path: readonly string[]) {
  return `config-difference-${encodeURIComponent(JSON.stringify([file, path]))}`;
}
export function openDifference(file: string, path: readonly string[]) {
  const element = document.getElementById(differenceId(file, path));
  if (!element) return;
  for (let parent: HTMLElement | null = element; parent; parent = parent.parentElement) {
    if (parent instanceof HTMLDetailsElement) parent.open = true;
  }
  if (element instanceof HTMLDetailsElement) element.open = true;
  element.scrollIntoView({ block: "nearest" });
  element.querySelector("summary")?.focus();
}

function DifferenceValues({
  difference,
  source,
  sensitive,
}: {
  difference: ConfigDifference;
  source: string;
  sensitive: boolean;
}) {
  const [revealed, setRevealed] = useState(false);
  const value = (present: boolean, content: unknown) =>
    !present
      ? "Not present"
      : sensitive && !revealed
        ? "••••••••"
        : JSON.stringify(content, null, 2);
  return (
    <div className={styles.differenceValues}>
      {sensitive && (
        <button
          type="button"
          className={styles.revealDiffButton}
          onClick={() => setRevealed((previous) => !previous)}
        >
          {revealed ? "Hide values" : "Show values"}
        </button>
      )}
      <div className={styles.differenceCompareGrid}>
        <div className={styles.differenceSide}>
          <div className={styles.differenceSideHeader}>
            <span className={styles.differenceBadgeNamed}>{source}</span>
            <span className={styles.differenceSourceName}>Named Config</span>
          </div>
          <pre
            className={`${styles.differenceCodeBlock} ${!difference.named_present ? styles.differenceAbsent : ""}`}
          >
            {value(difference.named_present, difference.named_value)}
          </pre>
        </div>
        <div className={styles.differenceSide}>
          <div className={styles.differenceSideHeader}>
            <span className={styles.differenceBadgeCurrent}>Current</span>
            <span className={styles.differenceSourceName}>Current Config</span>
          </div>
          <pre
            className={`${styles.differenceCodeBlock} ${!difference.current_present ? styles.differenceAbsent : ""}`}
          >
            {value(difference.current_present, difference.current_value)}
          </pre>
        </div>
      </div>
    </div>
  );
}

export function FieldDifferences({
  file,
  path,
  sensitive = false,
}: {
  file: string;
  path: string;
  sensitive?: boolean;
}) {
  const { result } = useConfigComparison();
  const differences =
    result?.files
      .find((entry) => entry.file === file)
      ?.differences.filter((entry) => {
        const changed = entry.path.join(".");
        return (
          path === "" ||
          changed === path ||
          path.startsWith(`${changed}.`) ||
          changed.startsWith(`${path}.`)
        );
      }) ?? [];
  if (!differences.length || !result) return null;
  return (
    <details className={styles.fieldDifference}>
      <summary>
        Differs from {file === "auth.json" ? "Current credentials" : "Current Config"}
      </summary>
      {differences.map((difference) => (
        <div key={JSON.stringify(difference.path)}>
          <code>{difference.path.join(".") || file}</code>
          <DifferenceValues
            difference={difference}
            source={result.source}
            sensitive={sensitive || difference.sensitive}
          />
        </div>
      ))}
    </details>
  );
}

export function FileDifferenceCount({ file }: { file: string }) {
  const { result } = useConfigComparison();
  const entries = result?.files.find((entry) => entry.file === file)?.differences ?? [];
  const count = file === "auth.json" ? Number(entries.length > 0) : entries.length;
  if (!count) return null;
  return (
    <button
      type="button"
      className={styles.differenceCount}
      aria-label={`Show ${count} differences for ${file}`}
      onClick={() => {
        const element = document.getElementById(differenceId(file, []));
        if (element instanceof HTMLDetailsElement) {
          element.open = true;
          const innerCards = element.querySelectorAll<HTMLDetailsElement>("details");
          for (const card of innerCards) card.open = true;
          element.querySelector("summary")?.focus();
        }
      }}
    >
      {count} {count === 1 ? "difference" : "differences"}
    </button>
  );
}

export function FileDifferences({
  file,
  onLocate,
}: {
  file: string;
  onLocate: (difference: ConfigDifference) => void;
}) {
  const { enabled, result, pending, error, dirty } = useConfigComparison();
  if (!enabled) return null;
  const comparison = result?.files.find((entry) => entry.file === file);
  if (pending)
    return (
      <span className={styles.comparisonStatus} role="status">
        <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
        Comparing{dirty ? " unsaved content" : ""}…
      </span>
    );
  const failure = error ?? comparison?.error;
  if (failure)
    return (
      <span className={`${styles.comparisonStatus} ${styles.comparisonError}`} role="status">
        <AlertTriangle size={iconSize.xs} aria-hidden="true" />
        Comparison unavailable: {failure}
      </span>
    );
  if (!comparison || !result) return null;
  const count =
    file === "auth.json"
      ? Number(comparison.differences.length > 0)
      : comparison.differences.length;
  if (!count)
    return (
      <span className={`${styles.comparisonStatus} ${styles.comparisonClean}`}>
        <Check size={iconSize.xs} aria-hidden="true" />
        No differences{dirty ? " · Unsaved content" : ""}
        {result.incomplete ? " · Other files unavailable" : ""}
      </span>
    );
  return (
    <details className={styles.fileDifferences} id={differenceId(file, [])}>
      <summary className={styles.fileDifferencesSummary}>
        <AlertTriangle size={iconSize.xs} aria-hidden="true" />
        {count} {count === 1 ? "difference" : "differences"}
        {dirty ? " · Unsaved content" : ""}
        {result.incomplete ? " · Comparison incomplete" : ""}
      </summary>
      <div className={styles.differenceList}>
        {comparison.differences.map((difference) => (
          <details
            key={JSON.stringify(difference.path)}
            id={differenceId(file, difference.path)}
            className={styles.differenceCard}
          >
            <summary className={styles.differenceCardSummary}>
              <span className={styles.differencePathBadge}>
                <code>{difference.path.join(".") || file}</code>
              </span>
            </summary>
            <div className={styles.differenceCardBody}>
              <DifferenceValues
                difference={difference}
                source={result.source}
                sensitive={difference.sensitive}
              />
              <div className={styles.differenceCardActions}>
                <button
                  type="button"
                  className={styles.locateRawButton}
                  onClick={() => onLocate(difference)}
                >
                  Locate in Raw
                </button>
              </div>
            </div>
          </details>
        ))}
      </div>
    </details>
  );
}
