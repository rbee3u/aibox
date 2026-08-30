import { describe, expect, it } from "vitest";
import {
  parseTenantSelectionValue,
  tenantSelectionFromValue,
  tenantSelectionValue,
  type TenantSelectionValue,
} from "@/domain/tenant";

describe("Tenant selection codec", () => {
  it.each<[string | null, TenantSelectionValue | null]>([
    ["host", "host"],
    ["managed:default", "managed:default"],
    ["managed:work-2", "managed:work-2"],
    ["managed:", null],
    ["managed:Upper", null],
    ["default", null],
    [null, null],
  ])("parses %s", (input, expected) => {
    expect(parseTenantSelectionValue(input)).toBe(expected);
  });

  it.each<TenantSelectionValue>(["host", "managed:default", "managed:work-2"])(
    "round-trips %s through the domain selection",
    (key) => {
      expect(tenantSelectionValue(tenantSelectionFromValue(key))).toBe(key);
    },
  );
});
