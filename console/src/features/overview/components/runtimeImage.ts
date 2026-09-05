import type { OverviewData } from "@/api/overview";
import type { Tone } from "@/features/overview/viewTypes";
import { formatBinaryByteSize } from "@/shared/lib/encoding";
import { formatTimestamp } from "@/shared/lib/format";

type ImageStatus = OverviewData["runtime_image"]["status"];

/** Runtime Image projections for the Overview status strip. */
export function imageTone(status?: ImageStatus): Tone {
  if (status === "built") return "good";
  if (status === "missing") return "warning";
  return "neutral";
}

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
