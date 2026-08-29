import { describe, expect, it } from "vitest";
import type { SessionListData } from "@/api/sessions";
import {
  aggregateSessionCatalog,
  groupSessionsForDeletion,
  sessionDialogSources,
  splitSessionResults,
} from "@/features/sessions/sessionCatalog";
import { sessionSource, sourcedSession } from "@/features/sessions/sessionSource";

function result(id: string, partial = false): SessionListData {
  return {
    sessions: [
      {
        id,
        display_id: id,
        start_ts: "2026-08-17T09:00:00Z",
        title: id,
        latest_message: "",
        message_count: 0,
        tool_count: 0,
        warnings: [],
      },
    ],
    warnings: [],
    partial,
  };
}

describe("Session catalog projection", () => {
  it("retains source identity and reports partial reads", () => {
    const source = sessionSource("managed:work", "codex");
    const failedSource = sessionSource("host", "claude");
    const { successes, failures } = splitSessionResults(
      [
        { status: "fulfilled", value: { source, result: result("ok") } },
        { status: "rejected", reason: new Error("unreadable") },
      ],
      [source, failedSource],
    );
    const catalog = aggregateSessionCatalog(successes, failures);
    expect(catalog.sessions[0].key).toContain("managed:work");
    expect(catalog.partial).toBe(true);
    expect(catalog.warnings).toContain("Host Tenant Claude: unreadable");
  });

  it("groups deletion requests by source and keeps source order stable", () => {
    const codex = sessionSource("managed:work", "codex");
    const claude = sessionSource("managed:work", "claude");
    const rows = [
      sourcedSession(claude, {
        id: "c",
        display_id: "c",
        start_ts: "",
        title: "",
        latest_message: "",
        message_count: 0,
        tool_count: 0,
        warnings: [],
      }),
      sourcedSession(codex, {
        id: "a",
        display_id: "a",
        start_ts: "",
        title: "",
        latest_message: "",
        message_count: 0,
        tool_count: 0,
        warnings: [],
      }),
      sourcedSession(codex, {
        id: "b",
        display_id: "b",
        start_ts: "",
        title: "",
        latest_message: "",
        message_count: 0,
        tool_count: 0,
        warnings: [],
      }),
    ];
    expect(groupSessionsForDeletion(rows).map((group) => [group.source.agent, group.ids])).toEqual([
      ["claude", ["c"]],
      ["codex", ["a", "b"]],
    ]);
    expect(sessionDialogSources(rows).map(({ count }) => count)).toEqual([1, 2]);
  });
});
