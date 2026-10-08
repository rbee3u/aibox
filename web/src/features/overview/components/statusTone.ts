import type { OverviewData } from "@/api/overview";
import type { Tone } from "@/features/overview/viewTypes";

type DockerStatus = OverviewData["docker"]["status"];
type ImageStatus = OverviewData["runtime_image"]["status"];

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
