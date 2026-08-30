import { describe, expect, it } from "vitest";
import { readSessionRoute, sessionLocation } from "@/features/sessions/route";

describe("Sessions route codec", () => {
  it("falls back to the Default Managed Tenant and Codex for an empty selection", () => {
    const route = readSessionRoute("");
    expect([...route.tenants]).toEqual(["managed:default"]);
    expect([...route.agents]).toEqual(["codex"]);
    expect(route.selection).toBeNull();
    expect(route.tab).toBe("conversation");
  });

  it("reads repeated Tenant and Coding Agent selections", () => {
    const route = readSessionRoute("?tenant=host&tenant=managed%3Awork&agent=claude&agent=codex");
    expect([...route.tenants].sort()).toEqual(["host", "managed:work"]);
    expect([...route.agents].sort()).toEqual(["claude", "codex"]);
  });

  it("drops unparsable Tenant keys and unknown Coding Agents", () => {
    const route = readSessionRoute("?tenant=managed%3A&tenant=Nope&agent=gemini");
    expect([...route.tenants]).toEqual(["managed:default"]);
    expect([...route.agents]).toEqual(["codex"]);
  });

  it("requires a complete selector before selecting a Session", () => {
    expect(readSessionRoute("?session=abc").selection).toBeNull();
    expect(readSessionRoute("?session_tenant=host&session=abc").selection).toBeNull();
    expect(
      readSessionRoute("?session_tenant=host&session_agent=claude&session=abc").selection,
    ).toEqual({ tenantSelectionValue: "host", agent: "claude", id: "abc" });
  });

  it("defaults an unknown tab to Conversation", () => {
    expect(readSessionRoute("?tab=nowhere").tab).toBe("conversation");
    expect(readSessionRoute("?tab=details").tab).toBe("details");
  });

  it("writes sorted Tenants and declared Coding Agent order", () => {
    const query = sessionLocation(new Set(["managed:work", "host"]), new Set(["claude", "codex"]));
    expect(query.toString()).toBe("tenant=host&tenant=managed%3Awork&agent=codex&agent=claude");
  });

  it("omits the tab unless a Session is selected and Details is active", () => {
    const filters = { tenants: new Set(["host" as const]), agents: new Set(["codex" as const]) };
    expect(sessionLocation(filters.tenants, filters.agents, null, "details").has("tab")).toBe(
      false,
    );
    const selectedSession = {
      tenantSelectionValue: "host" as const,
      agent: "codex" as const,
      id: "abc",
    };
    expect(
      sessionLocation(filters.tenants, filters.agents, selectedSession, "conversation").has("tab"),
    ).toBe(false);
    expect(
      sessionLocation(filters.tenants, filters.agents, selectedSession, "details").get("tab"),
    ).toBe("details");
  });

  it("round-trips a complete selection", () => {
    const search =
      "?tenant=host&agent=codex&session_tenant=host&session_agent=codex&session=abc&tab=details";
    const route = readSessionRoute(search);
    const query = sessionLocation(route.tenants, route.agents, route.selection, route.tab);
    expect(`?${query.toString()}`).toBe(search);
  });
});
