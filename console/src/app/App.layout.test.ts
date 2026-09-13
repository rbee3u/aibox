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
const css = fileSystem.readFileSync("src/app/App.module.css", "utf8");

describe("shell icon controls", () => {
  it("does not shrink Collapse sidebar and Open navigation below the 36px IconButton", () => {
    expect(css).not.toMatch(
      /\.collapseButton,\s*\.menuButton\s*\{[^}]*width:\s*30px;[^}]*height:\s*30px/s,
    );
    expect(css).toMatch(/\.collapseButton\s*\{[^}]*flex:\s*0 0 var\(--control-md\)/s);
  });

  it("uses one drawer scroll surface when the viewport is short", () => {
    expect(css).toContain("@media (max-width: 900px) and (max-height: 480px)");
    expect(css).toMatch(/\.sidebar\s*\{\s*display:\s*block;\s*overflow-y:\s*auto;\s*\}/s);
    expect(css).toMatch(
      /\.brand\s*\{[^}]*position:\s*sticky;[^}]*inset-block-start:\s*0;[^}]*background:\s*var\(--bg-shell\);[^}]*\}/s,
    );
    expect(css).toMatch(/\.moduleNav\s*\{\s*overflow-y:\s*visible;\s*\}/s);
  });
});
