import { describe, expect, it } from "vitest";
import {
  configLocation,
  readConfigRoute,
  type ConfigSelection,
} from "@/features/common/routes/configs";
import type { TenantSelection } from "@/domain/tenant";

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

  it("opens the Named Configs catalog without selecting Current Config", () => {
    expect(readConfigRoute("?tenant=host&agent=codex&named=1")).toEqual({
      tenant: { kind: "host" },
      agent: "codex",
      selection: { current: false, namedCatalog: true },
      file: null,
      detailOpen: false,
    });
  });

  it("lets an inspectable Config win over named=1", () => {
    expect(readConfigRoute("?named=1&current=1").selection).toEqual({ current: true });
    expect(readConfigRoute("?named=1&config=review").selection).toEqual({
      current: false,
      config: "review",
    });
    expect(readConfigRoute("?named=1&config=Review").selection).toEqual({
      current: false,
      namedCatalog: true,
    });
  });

  it("ignores a file on the Named Configs catalog", () => {
    expect(readConfigRoute("?named=1&file=config.toml").file).toBeNull();
    expect(readConfigRoute("?named=1&file=config.toml").detailOpen).toBe(false);
  });

  it("ignores a file without an open detail", () => {
    expect(readConfigRoute("?file=config.toml").file).toBeNull();
  });

  it("falls back to Codex for an unknown Agent", () => {
    expect(readConfigRoute("?agent=gemini").agent).toBe("codex");
  });

  it("always writes the Tenant and Agent", () => {
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

  it("writes the Named Configs catalog without a file", () => {
    expect(
      configLocation({ kind: "host" }, "codex", { current: false, namedCatalog: true }).toString(),
    ).toBe("tenant=host&agent=codex&named=1");
    expect(
      configLocation(
        { kind: "host" },
        "codex",
        { current: false, namedCatalog: true },
        "config.toml",
      ).has("file"),
    ).toBe(false);
  });

  it.each(
    (
      [
        { kind: "host" },
        { kind: "managed", name: "default" },
        { kind: "managed", name: "host" },
      ] satisfies TenantSelection[]
    ).flatMap((tenant) =>
      (["codex", "claude"] as const).flatMap((agent) =>
        (
          [
            { current: true },
            { current: false, config: "review" },
            { current: false, namedCatalog: true },
          ] satisfies ConfigSelection[]
        ).flatMap((selection) =>
          [null, "settings.json", "目录 +#&.json"].map((file) => ({
            tenant,
            agent,
            selection,
            file,
          })),
        ),
      ),
    ),
  )("round-trips Tenant $tenant, Agent $agent, selection $selection, file $file", (state) => {
    const query = configLocation(state.tenant, state.agent, state.selection, state.file);
    const route = readConfigRoute(query.toString());
    expect(route).toEqual({
      ...state,
      file: state.selection.namedCatalog ? null : state.file,
      detailOpen: !state.selection.namedCatalog,
    });
    expect(configLocation(route.tenant, route.agent, route.selection, route.file).toString()).toBe(
      query.toString(),
    );
  });
});
