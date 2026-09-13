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
const css = fileSystem.readFileSync("src/features/sessions/SessionPage.module.css", "utf8");
const component = fileSystem.readFileSync(
  "src/features/sessions/detail/SessionCopyValue.tsx",
  "utf8",
);

describe("Session detail copy controls", () => {
  it("takes the inline hit area from the primitive's own step", () => {
    expect(component).toMatch(/<IconButton\b[^>]*size="sm"/s);
  });

  /*
   * Sizing the control from this stylesheet is what once dropped it to 24px on
   * touch: a feature stylesheet loads after the primitive's, so an override
   * here silently outranks the coarse-pointer floor rather than layering on it.
   */
  it("does not size the control from the feature stylesheet", () => {
    const rule = /\.sessionCopyAction\s*\{([^}]*)\}/s.exec(css);
    expect(rule).not.toBeNull();
    expect(rule![1]).not.toMatch(/(?:min-)?(?:width|height)\s*:/);
    expect(rule![1]).not.toMatch(/flex\s*:/);
  });
});
