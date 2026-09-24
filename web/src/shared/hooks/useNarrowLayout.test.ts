import { describe, expect, it } from "vitest";
import { catalogMarksInspection } from "@/shared/hooks/useNarrowLayout";

describe("catalog inspection chrome", () => {
  it("marks the fallback row on desktop even when detail is not routed", () => {
    expect(catalogMarksInspection(false, false)).toBe(true);
    expect(catalogMarksInspection(false, true)).toBe(true);
  });

  it("marks a row on a one-pane layout only after detail opens", () => {
    expect(catalogMarksInspection(true, false)).toBe(false);
    expect(catalogMarksInspection(true, true)).toBe(true);
  });
});
