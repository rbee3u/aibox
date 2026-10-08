import samples from "@/api/generated/samples.json";
import { describe, expect, it } from "vitest";

describe("Rust-owned contract samples", () => {
  it("covers every Component kind and status and the Operation states", () => {
    expect(samples.component_rows).toHaveLength(8);
    expect(new Set(samples.component_rows.map((row) => row.kind)).size).toBe(8);
    expect(new Set(samples.component_rows.map((row) => row.status)).size).toBe(5);
    expect(samples.component_statuses).toContain(null);
    expect(new Set(samples.component_statuses.filter(Boolean)).size).toBe(5);
    expect(samples.operation_states).toEqual(["running", "succeeded", "failed", "cancelled"]);
  });
});
