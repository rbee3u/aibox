import { describe, expect, it } from "vitest";
import type { SessionRow } from "@/api/sessions";
import {
  compareSessions,
  focusTargetAfterSessionDelete,
  sessionSource,
  sourcedSession,
  visibleSessionListSource,
  visibleSessionSource,
} from "@/features/sessions/sessionSource";

function row(id: string, startTs: string): SessionRow {
  return {
    id,
    display_id: id.slice(0, 6),
    start_ts: startTs,
    title: id,
    latest_message: id,
    message_count: 1,
    tool_count: 0,
    warnings: [],
  };
}

describe("Session source vocabulary", () => {
  it("labels the Host Tenant differently in detail and list contexts", () => {
    const host = sessionSource("host", "codex");
    expect(visibleSessionSource(host)).toBe("Host Tenant Codex");
    expect(visibleSessionListSource(host)).toBe("Host Tenant Codex");
  });

  it("prefixes a Managed Tenant only in the detail label", () => {
    const managed = sessionSource("managed:work", "claude");
    expect(visibleSessionSource(managed)).toBe("Tenant work Claude");
    expect(visibleSessionListSource(managed)).toBe("work Claude");
    expect(managed.tenant).toEqual({ kind: "managed", name: "work" });
  });

  it("keys a row by its Tenant, Coding Agent, and Session id", () => {
    const source = sessionSource("managed:work", "codex");
    expect(sourcedSession(source, row("abc", "2026-08-17T09:00:00Z")).key).toBe(
      '["managed:work","codex","abc"]',
    );
  });

  it("orders newest first, then by Tenant, Coding Agent, and id", () => {
    const codexWork = sessionSource("managed:work", "codex");
    const claudeWork = sessionSource("managed:work", "claude");
    const older = sourcedSession(codexWork, row("a", "2026-08-17T08:00:00Z"));
    const newer = sourcedSession(codexWork, row("b", "2026-08-17T09:00:00Z"));
    expect([older, newer].sort(compareSessions).map((entry) => entry.id)).toEqual(["b", "a"]);

    const sameTime = [
      sourcedSession(codexWork, row("z", "2026-08-17T09:00:00Z")),
      sourcedSession(claudeWork, row("a", "2026-08-17T09:00:00Z")),
    ].sort(compareSessions);
    expect(sameTime.map((entry) => entry.source.agentLabel)).toEqual(["Claude", "Codex"]);
  });

  it("moves focus to the next row, then the previous one", () => {
    const source = sessionSource("host", "codex");
    const rows = ["a", "b", "c"].map((id) =>
      sourcedSession(source, row(id, "2026-08-17T09:00:00Z")),
    );
    expect(focusTargetAfterSessionDelete(rows, rows[0].key)).toBe(rows[1].key);
    expect(focusTargetAfterSessionDelete(rows, rows[2].key)).toBe(rows[1].key);
    expect(focusTargetAfterSessionDelete([rows[0]], rows[0].key)).toBeNull();
    expect(focusTargetAfterSessionDelete(rows, "missing")).toBeNull();
  });
});
