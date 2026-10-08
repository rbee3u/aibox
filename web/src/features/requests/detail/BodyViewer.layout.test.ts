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
const css = fileSystem.readFileSync("src/features/requests/detail/BodyViewer.module.css", "utf8");

describe("request body unredacted reminder", () => {
  it("uses muted metadata color instead of warning", () => {
    expect(css).toMatch(/\.sensitiveContext\s*\{[^}]*color:\s*var\(--muted\)/s);
  });
});
