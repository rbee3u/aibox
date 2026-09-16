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

describe("Session user prompt", () => {
  it("reads in the body face, keeping only the verbatim whitespace", () => {
    expect(css).toMatch(
      /\.plainText\s*\{[^}]*font-family:\s*inherit;[^}]*white-space:\s*pre-wrap/s,
    );
  });
});

describe("Agent Markdown", () => {
  it("steps headings by size where a reader scans", () => {
    expect(css).toMatch(/\.markdown h1\s*\{[^}]*font-size:\s*var\(--text-lg\)/s);
    expect(css).toMatch(/\.markdown h2\s*\{[^}]*font-size:\s*var\(--text-md\)/s);
  });
  it("lets table cells wrap inside the measure", () => {
    const cells = /\.markdown th,\s*\.markdown td\s*\{([^}]*)\}/s.exec(css);
    expect(cells).not.toBeNull();
    expect(cells![1]).not.toMatch(/white-space:\s*nowrap/);
  });
});
