import type { OverviewData } from "@/api/overview";
import type { Tone } from "@/features/overview/viewTypes";

/** Runtime Image projections for the Runtime section. */
export function imageTone(status?: OverviewData["runtime_image"]["status"]): Tone {
  if (status === "built") return "good";
  if (status === "missing") return "warning";
  return "neutral";
}

export function shortImageId(id: string | null | undefined): string {
  if (!id) return "—";
  const value = id.startsWith("sha256:") ? id.slice(7) : id;
  return value.slice(0, 12);
}
