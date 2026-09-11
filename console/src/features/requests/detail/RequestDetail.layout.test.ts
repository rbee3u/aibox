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
  "src/features/requests/detail/RequestDetail.module.css",
  "utf8",
);

describe("Request summary copy controls", () => {
  it("keeps Session ID copy on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(
      /\.copySession\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
  });
});
