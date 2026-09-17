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

const css = fileSystem.readFileSync("src/features/requests/catalog/RequestList.module.css", "utf8");
const confirmDialogCss = fileSystem.readFileSync("src/shared/ui/ConfirmDialog.module.css", "utf8");

describe("RequestList layout and responsive behaviors", () => {
  it("hides page turn button labels on narrow containers to prevent pagination text truncation", () => {
    const container = /@container\s*\(max-width:\s*430px\)\s*\{([^}]*)\}/s.exec(css);
    expect(container).not.toBeNull();
    expect(css).toMatch(/\.pageTurnLabel\s*\{[^}]*display:\s*none/s);
  });

  it("distributes timing and timestamp across full width with space-between on narrow containers", () => {
    expect(css).toMatch(/\.timingMetadata\s*\{[^}]*justify-content:\s*space-between/s);
    expect(css).toMatch(/\.timingMetadata\s*\{[^}]*justify-self:\s*stretch/s);
  });

  it("provides fullWidthFact rule spanning all grid columns in ConfirmDialog", () => {
    expect(confirmDialogCss).toMatch(/\.fullWidthFact\s*\{[^}]*grid-column:\s*1\s*\/\s*-1/s);
  });

  it("uses refined 18px checkbox styling with comfortable right inset", () => {
    expect(css).toMatch(
      /\.selectionIndicator\s*\{[^}]*right:\s*12px;[^}]*width:\s*18px;[^}]*height:\s*18px/s,
    );
    expect(css).toMatch(/\.selectionRecord \.rowButton\s*\{[^}]*padding-right:\s*38px/s);
  });
});
