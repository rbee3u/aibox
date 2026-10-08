import styles from "@/features/requests/detail/DiagnosticGroup.module.css";
import { CircleAlert, TriangleAlert } from "lucide-react";

import type { AssessmentFinding } from "@/api/requests";

import { elapsedNsMs } from "@/features/requests/summary";

import { duration } from "@/shared/lib/format";

import { assessmentPrimaryLabel } from "@/features/requests/statusPresentation";

import { iconSize } from "@/shared/icons/iconSizes";

const PHASE_LABELS: Record<string, string> = {
  request: "Request",
  response: "Response",
  model_api: "Model API",
  recording: "Recording",
  transport: "Transport",
  proxy: "Proxy",
};

function formatPhase(phase: string): string {
  const normalized = phase.trim().toLowerCase();
  if (PHASE_LABELS[normalized]) return PHASE_LABELS[normalized];
  return normalized.replace(/[_-]+/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

export function DiagnosticGroup({
  title,
  entries,
  tone,
}: {
  title: string;
  entries: AssessmentFinding[];
  tone: "error" | "warning";
}) {
  if (entries.length === 0) return null;
  const Icon = tone === "error" ? CircleAlert : TriangleAlert;
  return (
    <section
      className={`${styles.diagnosticGroup} ${
        tone === "error" ? styles.errorDiagnostics : styles.warningDiagnostics
      }`}
      aria-label={title}
    >
      <h3>
        {title} <span>{entries.length}</span>
      </h3>
      <div className={styles.diagnosticList}>
        {entries.map((entry, index) => (
          <article
            className={styles.diagnosticItem}
            key={`${entry.source}-${entry.kind}-${entry.at_ns}-${index}`}
          >
            <div className={styles.diagnosticMeta}>
              <Icon className={styles.diagnosticIcon} size={iconSize.xs} aria-hidden="true" />
              <strong>{assessmentPrimaryLabel(entry)}</strong>
              {entry.phase && (
                <span className={styles.diagnosticPhase}>{formatPhase(entry.phase)}</span>
              )}
              {entry.at_ns && (
                <span className={styles.diagnosticTime}>+{duration(elapsedNsMs(entry.at_ns))}</span>
              )}
            </div>
            <p>{entry.message}</p>
          </article>
        ))}
      </div>
    </section>
  );
}
