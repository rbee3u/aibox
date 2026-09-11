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
const css = fileSystem.readFileSync("src/shared/ui/IconButton.module.css", "utf8");

describe("icon button sizing", () => {
  it("offers a text-sized step below the default slot size", () => {
    expect(css).toMatch(/\.button\s*\{[^}]*width:\s*var\(--control-md\)/s);
    expect(css).toMatch(/\.sm\s*\{[^}]*width:\s*var\(--control-xs\)/s);
  });

  /*
   * The floor has to be stated last and in this file. A feature stylesheet
   * loads after the primitive's, so sizing an icon button from a call site
   * outranks the coarse-pointer rule instead of layering on it — which is how
   * the Session copy control ended up at 24px on touch.
   */
  it("keeps the coarse-pointer floor over both steps, stated last", () => {
    const coarse = /@media\s*\(pointer:\s*coarse\)\s*\{(.*)$/s.exec(css);
    expect(coarse).not.toBeNull();
    expect(coarse![1]).toMatch(/\.button\s*,\s*\.button\.sm\s*\{[^}]*min-height:\s*44px/s);
    expect(css.indexOf("@media (pointer: coarse)")).toBeGreaterThan(css.indexOf(".sm {"));
  });
});
