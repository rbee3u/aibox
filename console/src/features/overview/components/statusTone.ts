import type { OverviewData } from "@/api/overview";
import type { Tone } from "@/features/overview/viewTypes";

type DockerStatus = OverviewData["docker"]["status"];
type ImageStatus = OverviewData["runtime_image"]["status"];

/**
 * Tone projections for the Overview status strip.
 *
 * The strip exists to answer "is this healthy", so a healthy fact reports
 * `good` and spends the status vocabulary saying so. `neutral` is reserved for
 * a fact still resolving, or one the Service could not report at all — a fact
 * that has reported and is fine is not neutral.
 */
export function dockerTone(status?: DockerStatus): Tone {
  if (status === "available") return "good";
  if (status === "unavailable") return "error";
  return "neutral";
}

export function imageTone(status?: ImageStatus): Tone {
  if (status === "built") return "good";
  if (status === "missing") return "warning";
  return "neutral";
}
