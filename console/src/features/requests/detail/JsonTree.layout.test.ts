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
const css = fileSystem.readFileSync("src/features/requests/detail/JsonTree.module.css", "utf8");

describe("JSON pretty tree controls", () => {
  it("keeps expand and copy on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(
      /\.jsonToggle\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).toMatch(
      /\.jsonCopy\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).not.toMatch(/\.jsonToggle\s*\{[^}]*width:\s*20px/s);
    expect(css).not.toMatch(/\.jsonCopy\s*\{[^}]*width:\s*22px/s);
  });
});
