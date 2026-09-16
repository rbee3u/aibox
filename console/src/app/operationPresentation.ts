import { Ban, Check, CircleX, LoaderCircle } from "lucide-react";
import type { ComponentType } from "react";

import type { Operation, OperationState } from "@/api/operations";
import type { StatusTone } from "@/shared/ui/StatusBadge";

interface OperationStatePresentation {
  /** Sentence-case label; the wire spelling is an enum, not a reading. */
  label: string;
  tone: StatusTone;
  icon: ComponentType<{ size?: number; className?: string }>;
}

/**
 * What one Operation state looks like and says. A Coding Agent install that
 * fails and one that succeeds have to be distinguishable at a glance, so each
 * terminal state owns a tone and its own mark rather than sharing a stop sign.
 */
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
 * Milliseconds the Operation has been running, or ran for. A running Operation
 * measures against the caller's clock so the panel can tick; a finished one is
 * fixed. Unparsable timestamps report `null` rather than a wrong number.
 */
export function operationElapsedMs(operation: Operation, now: number): number | null {
  const started = Date.parse(operation.started_at);
  if (!Number.isFinite(started)) return null;
  const ended = operation.ended_at ? Date.parse(operation.ended_at) : null;
  const end = ended !== null && Number.isFinite(ended) ? ended : now;
  return Math.max(0, end - started);
}
