import { describe, expect, it } from "vitest";
import type { SessionListData } from "@/api/sessions";
import {
  messageCountLabel,
  projectSessionCatalog,
  toolCountLabel,
} from "@/features/sessions/sessionCatalog";
import { sessionSource } from "@/features/sessions/sessionSource";

function result(id: string, partial = false): SessionListData {
  return {
    sessions: [
      {
        id,
        display_id: id,
        start_ts: "2026-08-17T09:00:00Z",
        title: id,
        latest_message: "",
        message_count: 1,
        tool_count: 2,
        warnings: [],
      },
    ],
    warnings: ["warning-1"],
    partial,
  };
}

describe("Session catalog projection", () => {
  it("projects a single source read and preserves warnings and partial flags", () => {
    const source = sessionSource("managed:work", "codex");
    const catalog = projectSessionCatalog(source, result("ok", true));
    expect(catalog.sessions[0].key).toBe('["managed:work","codex","ok"]');
    expect(catalog.partial).toBe(true);
    expect(catalog.warnings).toEqual(["warning-1"]);
  });

  it("formats singular and plural message and tool count labels", () => {
    expect(messageCountLabel(1)).toBe("1 message");
    expect(messageCountLabel(2)).toBe("2 messages");
    expect(toolCountLabel(1)).toBe("1 tool");
    expect(toolCountLabel(0)).toBe("0 tools");
  });
});
