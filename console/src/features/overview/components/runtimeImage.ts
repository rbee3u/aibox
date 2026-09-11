import type { OverviewData } from "@/api/overview";
import { formatBinaryByteSize } from "@/shared/lib/encoding";
import { formatTimestamp } from "@/shared/lib/format";

type ImageStatus = OverviewData["runtime_image"]["status"];

/** Runtime Image identity and build-action projections for Overview. */
export function shortImageId(id: string | null | undefined): string {
  if (!id) return "—";
  const value = id.startsWith("sha256:") ? id.slice(7) : id;
  return value.slice(0, 12);
}

export function imageTitle(image?: OverviewData["runtime_image"]): string {
  if (!image) return "Resolving image";
  return [
    image.reference,
    image.id ? shortImageId(image.id) : null,
    image.created_at ? formatTimestamp(image.created_at) : null,
    image.size_bytes == null ? null : formatBinaryByteSize(image.size_bytes),
  ]
    .filter((part) => part)
    .join(" · ");
}

/** Build is the page primary only while the Runtime Image still needs creating. */
export function buildActionTone(status?: ImageStatus): "primarySoft" | "secondary" {
  return cachelessBuildInline(status) ? "primarySoft" : "secondary";
}

/** Keep the cacheless rebuild inline only while the image still needs creating. */
export function cachelessBuildInline(status?: ImageStatus): boolean {
  return status === "missing" || status === "unknown";
}

export function formatDuration(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  if (days) return `${days}d ${hours}h`;
  if (hours) return `${hours}h ${minutes}m`;
  if (minutes) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}
