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
const css = fileSystem.readFileSync("src/shared/ui/FormControls.module.css", "utf8");

describe("form toggles", () => {
  it("keeps the checkbox mark on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(/\.toggle\s*\{[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s);
    expect(css).toMatch(
      /\.toggleMark\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).not.toMatch(/\.toggle\s*\{[^}]*min-height:\s*20px/s);
    expect(css).not.toMatch(/\.toggleMark\s*\{[^}]*width:\s*16px/s);
  });
});
