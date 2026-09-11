import { describe, expect, it } from "vitest";

const fileSystem = (
  globalThis as typeof globalThis & {
    process: {
      getBuiltinModule(name: "fs"): {
        readFileSync(path: string, encoding: "utf8"): string;
      };
    };
  }
).process.getBuiltinModule("fs");
const css = fileSystem.readFileSync("src/shared/ui/layout/catalog.module.css", "utf8");

describe("catalog toolbar", () => {
  it("wraps Tenant and Agent filters before shrinking current values unreadably", () => {
    expect(css).toMatch(/\.toolbar:not\(\.selectionBar\)\s*\{[^}]*flex-wrap:\s*wrap/s);
    expect(css).toMatch(/catalog-filter-control-max-width/);
  });
});

describe("catalog protected rows", () => {
  it("does not wash protected rows on the one-pane picker", () => {
    expect(css).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.splitLayout:not\(\.showsDetail\) \.rowProtected:not\(\.rowSelected\):not\(\.rowInspected\)\s*\{[^}]*background:\s*var\(--surface\)/s,
    );
  });
});

describe("catalog create dialogs", () => {
  it("uses Console ink instead of the native dialog CanvasText color", () => {
    expect(css).toMatch(/\.dialog\s*\{[^}]*color:\s*var\(--ink\)/s);
  });
});
