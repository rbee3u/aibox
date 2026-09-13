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
    expect(css).not.toMatch(/\.sensitiveContext\s*\{[^}]*color:\s*var\(--warning\)/s);
  });
});

describe("SSE pretty event copy", () => {
  it("keeps the row copy control on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(
      /\.eventCopy\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
    expect(css).not.toMatch(/\.eventCopy\s*\{[^}]*width:\s*28px/s);
  });
});
