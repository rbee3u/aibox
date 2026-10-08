import { Ban, Check, CircleX, LoaderCircle } from "lucide-react";
import type { ComponentType } from "react";

import type { Operation, OperationState } from "@/api/operations";
import type { StatusTone } from "@/shared/ui/StatusBadge";

interface OperationStatePresentation {
  label: string;
  tone: StatusTone;
  icon: ComponentType<{ size?: number; className?: string }>;
}

const STATE_PRESENTATION: Record<OperationState, OperationStatePresentation> = {
  running: { label: "Running", tone: "active", icon: LoaderCircle },
  succeeded: { label: "Succeeded", tone: "good", icon: Check },
  failed: { label: "Failed", tone: "error", icon: CircleX },
  cancelled: { label: "Cancelled", tone: "warning", icon: Ban },
};

export function operationPresentation(state: OperationState): OperationStatePresentation {
  return STATE_PRESENTATION[state];
}

/**
 * Use `ended_at` when valid, otherwise the caller's clock. An invalid start
 * returns `null`; negative durations clamp to zero.
 */
export function operationElapsedMs(operation: Operation, now: number): number | null {
  const started = Date.parse(operation.started_at);
  if (!Number.isFinite(started)) return null;
  const ended = operation.ended_at ? Date.parse(operation.ended_at) : null;
  const end = ended !== null && Number.isFinite(ended) ? ended : now;
  return Math.max(0, end - started);
}
