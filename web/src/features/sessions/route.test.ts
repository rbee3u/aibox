import { describe, expect, it } from "vitest";
import { readSessionRoute, sessionLocation } from "@/features/sessions/route";

describe("Sessions route codec", () => {
  it("falls back to the Default Managed Tenant and Codex for an empty selection", () => {
    const route = readSessionRoute("");
    expect(route.tenant).toEqual({ kind: "managed", name: "default" });
    expect(route.agent).toBe("codex");
    expect(route.sessionId).toBeNull();
    expect(route.tab).toBe("conversation");
  });

  it("reads single Tenant and Agent selections and picks the first when repeated", () => {
    const route = readSessionRoute("?tenant=host&tenant=managed%3Awork&agent=claude&agent=codex");
    expect(route.tenant).toEqual({ kind: "host" });
    expect(route.agent).toBe("claude");
  });

  it("drops unparsable Tenant keys and unknown Agents", () => {
    const route = readSessionRoute("?tenant=managed%3A&tenant=Nope&agent=gemini");
    expect(route.tenant).toEqual({ kind: "managed", name: "default" });
    expect(route.agent).toBe("codex");
  });

  it("reads a selected Session from the session param", () => {
    expect(readSessionRoute("?session=abc").sessionId).toBe("abc");
    expect(readSessionRoute("").sessionId).toBeNull();
  });

  it("defaults an unknown tab to Conversation", () => {
    expect(readSessionRoute("?tab=nowhere").tab).toBe("conversation");
    expect(readSessionRoute("?tab=details").tab).toBe("details");
  });

  it("writes Tenant and Agent parameters", () => {
    const query = sessionLocation({ kind: "managed", name: "work" }, "claude");
    expect(query.toString()).toBe("tenant=managed%3Awork&agent=claude");
  });

  it("omits the tab unless a Session is selected and Details is active", () => {
    expect(sessionLocation({ kind: "host" }, "codex", null, "details").has("tab")).toBe(false);
    expect(sessionLocation({ kind: "host" }, "codex", "abc", "conversation").has("tab")).toBe(
      false,
    );
    expect(sessionLocation({ kind: "host" }, "codex", "abc", "details").get("tab")).toBe("details");
  });

  it("round-trips a complete selection", () => {
    const search = "?tenant=host&agent=codex&session=abc&tab=details";
    const route = readSessionRoute(search);
    const query = sessionLocation(route.tenant, route.agent, route.sessionId, route.tab);
    expect(`?${query.toString()}`).toBe(search);
  });
});
