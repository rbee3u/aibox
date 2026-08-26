import { describe, expect, it } from "vitest";
import { configLocation, readConfigRoute } from "@/features/configs/route";

describe("Configs route codec", () => {
  it("defaults to the Default Managed Tenant with Codex and Current Config", () => {
    expect(readConfigRoute("")).toEqual({
      tenant: { kind: "managed", name: "default" },
      agent: "codex",
      selection: { current: true },
      file: null,
      detailOpen: false,
    });
  });

  it("reads the Host Tenant and Claude", () => {
    const route = readConfigRoute("?tenant=host&agent=claude&current=1&file=settings.json");
    expect(route.tenant).toEqual({ kind: "host" });
    expect(route.agent).toBe("claude");
    expect(route.selection).toEqual({ current: true });
    expect(route.file).toBe("settings.json");
    expect(route.detailOpen).toBe(true);
  });

  it("selects a Named Config only when its name is a DNS label", () => {
    expect(readConfigRoute("?config=review").selection).toEqual({
      current: false,
      config: "review",
    });
    expect(readConfigRoute("?config=Review").selection).toEqual({ current: true });
    expect(readConfigRoute("?config=").selection).toEqual({ current: true });
  });

  it("lets Current Config win over a Named Config", () => {
    expect(readConfigRoute("?current=1&config=review").selection).toEqual({ current: true });
  });

  it("ignores a file without an open detail", () => {
    expect(readConfigRoute("?file=config.toml").file).toBeNull();
  });

  it("falls back to Codex for an unknown Coding Agent", () => {
    expect(readConfigRoute("?agent=gemini").agent).toBe("codex");
  });

  it("always writes the Tenant and Coding Agent", () => {
    expect(configLocation({ kind: "host" }, "claude", null).toString()).toBe(
      "tenant=host&agent=claude",
    );
  });

  it("writes the selected Config and its file", () => {
    const tenant = { kind: "managed", name: "work" } as const;
    expect(configLocation(tenant, "codex", { current: true }, "config.toml").toString()).toBe(
      "tenant=managed%3Awork&agent=codex&current=1&file=config.toml",
    );
    expect(
      configLocation(tenant, "codex", { current: false, config: "review" }, "auth.json").toString(),
    ).toBe("tenant=managed%3Awork&agent=codex&config=review&file=auth.json");
  });

  it("drops a file when nothing is selected", () => {
    expect(configLocation({ kind: "host" }, "codex", null, "config.toml").has("file")).toBe(false);
  });

  it("round-trips a complete selection", () => {
    const search = "?tenant=managed%3Awork&agent=claude&config=review&file=settings.json";
    const route = readConfigRoute(search);
    const query = configLocation(route.tenant, route.agent, route.selection, route.file);
    expect(`?${query.toString()}`).toBe(search);
  });
});
