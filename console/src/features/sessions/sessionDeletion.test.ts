import { describe, expect, it } from "vitest";

import type { SessionRow } from "@/api/sessions";
import { sessionDeletionFacts } from "@/features/sessions/sessionDeletion";
import { sessionSource, sourcedSession } from "@/features/sessions/sessionSource";

const firstSession = {
  id: "11111111-1111-1111-1111-111111111111",
  display_id: "111111111111",
  start_ts: "2026-08-17T09:00:00Z",
  title: "First prompt",
  latest_message: "First prompt",
  message_count: 1,
  tool_count: 0,
  warnings: [],
} satisfies SessionRow;

describe("sessionDeletionFacts", () => {
  it("restates the catalog row for a Managed Tenant Session", () => {
    const session = sourcedSession(sessionSource("managed:default", "codex"), firstSession);
    expect(sessionDeletionFacts(session)).toEqual([
      { label: "Session", value: "First prompt" },
      { label: "Source", value: "default Codex" },
      { label: "Started", value: "2026-08-17 17:00:00" },
    ]);
  });

  it("uses the catalog headline when the native title is a skill path", () => {
    const session = sourcedSession(sessionSource("host", "codex"), {
      ...firstSession,
      title: "[$improve-unit-tests](/Users/rbee3u/.agents/skills/x/SKILL.md)",
      latest_message: "已完善 SSE 观察上限相关单元测试",
    });
    expect(sessionDeletionFacts(session)).toEqual([
      { label: "Session", value: "已完善 SSE 观察上限相关单元测试" },
      { label: "Source", value: "Host Tenant Codex" },
      { label: "Started", value: "2026-08-17 17:00:00" },
    ]);
  });
});
