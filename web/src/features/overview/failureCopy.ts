/**
 * Recognize actionable failures without guessing unknown causes.
 * Keep raw error text behind a disclosure as diagnostic evidence.
 */

/** Which read produced the failure, used when the cause is not recognized. */
export type FailureSource = "docker" | "service" | "topology" | "build";

export interface FailureCopy {
  /** The sentence shown in the row, in the Console's own voice. */
  detail: string;
  /** The original text, kept as evidence behind a disclosure. */
  technical?: string;
}

const UNRECOGNIZED: Record<FailureSource, string> = {
  docker: "Docker is unavailable.",
  service: "Service status could not be read.",
  topology: "Tenant resources could not be inspected.",
  build: "The Runtime Image build failed.",
};

/**
 * Match daemon failure first: its missing socket can resemble a missing binary.
 */
function recognize(raw: string): string | null {
  const text = raw.toLowerCase();
  if (text.includes("cannot connect to the docker daemon"))
    return "The Docker daemon is not running. Start Docker, then refresh.";
  if (text.includes("is docker installed?") && text.includes("no such file or directory"))
    return "Docker was not found. Install Docker, then refresh.";
  if (text.includes("failed to fetch") || text.includes("networkerror"))
    return "The AIBox Service could not be reached. It may have stopped.";
  if (text.includes("permission denied"))
    return "AIBox is not allowed to read this. Check the permissions on $AIBOX_ROOT.";
  return null;
}

export function explainFailure(source: FailureSource, raw: string | null): FailureCopy {
  const text = raw?.trim();
  if (!text) return { detail: UNRECOGNIZED[source] };
  return { detail: recognize(text) ?? UNRECOGNIZED[source], technical: text };
}
