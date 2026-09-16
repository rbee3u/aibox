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
const css = fileSystem.readFileSync("src/shared/ui/IssueIndicator.module.css", "utf8");

describe("issue indicators", () => {
  it("keeps help on the same 24px hit area as error and warning markers", () => {
    expect(css).toMatch(
      /\.indicator\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).not.toMatch(/\.help\s*\{[^}]*width:\s*20px/s);
  });
});
