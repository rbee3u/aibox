import samples from "@/api/generated/samples.json";
import { describe, expect, it } from "vitest";
import { decodeComponentRow } from "@/api/tenants";
import type { ComponentRow } from "@/api/generated/wire";

describe("Rust-owned contract samples", () => {
  it("passes Component samples through the production adapter normalization", () => {
    expect(samples.component_rows).toHaveLength(8);
    expect(
      samples.component_rows.map((row) => decodeComponentRow(row as unknown as ComponentRow)),
    ).toEqual(samples.component_rows);
    expect(new Set(samples.component_rows.map((row) => row.kind)).size).toBe(8);
    expect(new Set(samples.component_rows.map((row) => row.status)).size).toBe(5);
    expect(samples.component_statuses).toContain(null);
    expect(new Set(samples.component_statuses.filter(Boolean)).size).toBe(5);
    expect(samples.operation_states).toEqual(["running", "succeeded", "failed", "cancelled"]);
  });
});
