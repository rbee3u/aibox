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
const css = fileSystem.readFileSync(
  "src/features/sessions/detail/SessionMessageContent.module.css",
  "utf8",
);

describe("Session conversation code copy", () => {
  it("keeps Copy code on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(
      /\.copyCode\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).not.toMatch(/\.copyCode\s*\{[^}]*width:\s*26px/s);
  });
});
