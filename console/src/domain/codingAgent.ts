export type CodingAgentKind = "codex" | "claude";

export const CODING_AGENTS: readonly CodingAgentKind[] = ["codex", "claude"];

export function isCodingAgentKind(value: string): value is CodingAgentKind {
  return CODING_AGENTS.includes(value as CodingAgentKind);
}
