/**
 * Human copy for the failures the Console can only observe as error text.
 *
 * Conditions derived from the topology snapshot already read as sentences —
 * "shadow1 · Codex · Current Config differs" — because the Console knows what
 * they mean. Failures arrive instead as a Rust `anyhow` context chain or a
 * browser fetch rejection, and rendering those verbatim put an internal
 * operation name, a developer aside, and an errno in the one panel whose job
 * is telling an operator what to do next.
 *
 * Recognition is deliberately narrow: four causes that name a real next step.
 * Anything else keeps a per-source lead sentence, which states what failed
 * without guessing why. The raw text is never dropped — it moves behind a
 * disclosure as evidence.
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
 * A dead daemon and a missing binary are the two Docker failures an operator
 * actually hits, and they need opposite actions, so they are told apart rather
 * than folded into one "Docker is unavailable".
 *
 * The daemon message is matched before the missing-binary one because the
 * daemon case reports through a failed `docker image inspect` whose own text
 * can mention a missing socket file, which would otherwise read as the binary
 * being absent.
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

/**
 * The sentence for a failure, plus the raw text when it is worth keeping.
 *
 * The raw text is suppressed only when it would repeat the sentence, which
 * happens when there is no raw text at all.
 */
export function explainFailure(source: FailureSource, raw: string | null): FailureCopy {
  const text = raw?.trim();
  if (!text) return { detail: UNRECOGNIZED[source] };
  return { detail: recognize(text) ?? UNRECOGNIZED[source], technical: text };
}
