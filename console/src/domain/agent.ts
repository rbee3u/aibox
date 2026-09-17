export type AgentKind = "codex" | "claude";

export const AGENTS: readonly AgentKind[] = ["codex", "claude"];

export function isAgentKind(value: string): value is AgentKind {
  return AGENTS.includes(value as AgentKind);
}
