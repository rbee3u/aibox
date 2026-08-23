import { describe, expect, it } from "vitest";
import {
  parseTenantSelectionKey,
  tenantBody,
  tenantQuery,
  tenantSelectionFromKey,
  tenantSelectionValue,
  type TenantSelectionKey,
} from "./tenantSelection";

describe("Tenant selection codec", () => {
  it.each<[string | null, TenantSelectionKey | null]>([
    ["host", "host"],
    ["managed:default", "managed:default"],
    ["managed:work-2", "managed:work-2"],
    ["managed:", null],
    ["managed:Upper", null],
    ["default", null],
    [null, null],
  ])("parses %s", (input, expected) => {
    expect(parseTenantSelectionKey(input)).toBe(expected);
  });

  it.each<TenantSelectionKey>(["host", "managed:default", "managed:work-2"])(
    "round-trips %s through the domain selection",
    (key) => {
      expect(tenantSelectionValue(tenantSelectionFromKey(key))).toBe(key);
    },
  );

  it("uses the same encoding for query and wire body", () => {
    const tenant = { kind: "managed", name: "work" } as const;
    expect(tenantQuery(tenant).toString()).toBe("tenant=managed%3Awork");
    expect(tenantBody(tenant)).toEqual({ tenant: "managed:work" });
  });
});
