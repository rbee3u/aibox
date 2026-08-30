import { AlertTriangle, Hammer, Image, LoaderCircle, RefreshCw, Server } from "lucide-react";
import type { Operation } from "@/api/operations";
import { formatBinaryByteSize } from "@/shared/lib/encoding";
import type { OverviewData } from "@/api/overview";
import { RuntimeStatus } from "@/features/overview/components/OverviewFacts";
import { capitalize, imageTone, shortImageId } from "@/features/overview/topology/topologyModel";
import { formatTimestamp } from "@/shared/lib/format";
import { ActionButton } from "@/shared/ui/ActionButton";
import { SectionHeader } from "@/shared/ui/SurfacePrimitives";
import styles from "@/features/overview/OverviewPage.module.css";

interface RuntimeSectionProps {
  overview: OverviewData | null;
  /** The running Operation, when one owns the Runtime Image. */
  operation: Operation | null;
  buildDisabled: boolean;
  /** Explains a disabled build, and is announced through aria-describedby. */
  buildUnavailableReason: string | null;
  onBuild: (force: boolean) => void;
}

/**
 * Overview is the only Runtime Image build entry point, so this section reports
 * Docker availability and exact local image state beside its two build actions.
 */
export function RuntimeSection({
  overview,
  operation,
  buildDisabled,
  buildUnavailableReason,
  onBuild,
}: RuntimeSectionProps) {
  const operationRunning = operation?.state === "running";
  return (
    <section className={styles.runtimeSection} aria-labelledby="runtime-title">
      <SectionHeader
        className={styles.sectionHeading}
        eyebrow="Docker execution"
        title="Runtime"
        id="runtime-title"
      />
      <div className={styles.runtimeGrid}>
        <RuntimeStatus
          icon={<Server size={18} />}
          label="Docker"
          value={capitalize(overview?.docker.status ?? "checking")}
          detail={overview?.docker.error ?? "Docker CLI and daemon"}
          tone={overview?.docker.status === "available" ? "good" : overview ? "error" : "neutral"}
        />
        <RuntimeStatus
          icon={<Image size={18} />}
          label="Runtime Image"
          value={capitalize(overview?.runtime_image.status ?? "checking")}
          detail={overview?.runtime_image.reference ?? "Resolving image"}
          tone={imageTone(overview?.runtime_image.status)}
        />
        <dl className={styles.imageMetadata}>
          <div>
            <dt>Image ID</dt>
            <dd title={overview?.runtime_image.id ?? undefined}>
              {shortImageId(overview?.runtime_image.id)}
            </dd>
          </div>
          <div>
            <dt>Created</dt>
            <dd>
              {overview?.runtime_image.created_at
                ? formatTimestamp(overview.runtime_image.created_at)
                : "—"}
            </dd>
          </div>
          <div>
            <dt>Size</dt>
            <dd>
              {overview?.runtime_image.size_bytes == null
                ? "—"
                : formatBinaryByteSize(overview.runtime_image.size_bytes)}
            </dd>
          </div>
        </dl>
        <div className={styles.runtimeActions}>
          {operationRunning && operation && (
            <span className={styles.operationState} title={operation.kind}>
              <LoaderCircle className="spin" size={14} /> {operation.kind}
            </span>
          )}
          <ActionButton
            tone="primary"
            disabled={buildDisabled}
            aria-describedby={buildUnavailableReason ? "runtime-build-unavailable" : undefined}
            title={buildUnavailableReason ?? "Build Runtime Image using Docker cache"}
            onClick={() => onBuild(false)}
          >
            <Hammer size={15} /> Build
          </ActionButton>
          <ActionButton
            disabled={buildDisabled}
            aria-describedby={buildUnavailableReason ? "runtime-build-unavailable" : undefined}
            title={
              buildUnavailableReason ??
              "Re-run every layer without cache and pull a fresh base image"
            }
            onClick={() => onBuild(true)}
          >
            <RefreshCw size={15} /> Build without cache
          </ActionButton>
        </div>
      </div>
      {buildUnavailableReason && (
        <p id="runtime-build-unavailable" className={styles.buildUnavailable} role="status">
          {buildUnavailableReason}
        </p>
      )}
      {overview?.runtime_image.detail && (
        <div className={styles.runtimeNotice} role="status">
          <AlertTriangle size={15} /> {overview.runtime_image.detail}
        </div>
      )}
    </section>
  );
}
