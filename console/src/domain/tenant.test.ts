import { describe, expect, it } from "vitest";
import {
  parseTenantSelectionKey,
  tenantSelectionFromKey,
  tenantSelectionValue,
  type TenantSelectionKey,
} from "@/domain/tenant";

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
});
