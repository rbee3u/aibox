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

  it("provides hover, active, and focus micro-interactions with rounded corners", () => {
    expect(css).toMatch(/\.eventCopy:hover\s*\{[^}]*background:\s*var\(--surface-hover\);/s);
    expect(css).toMatch(/\.eventCopy:active\s*\{[^}]*background:\s*var\(--surface-pressed\);/s);
    expect(css).toMatch(/\.eventCopy:focus-visible\s*\{[^}]*outline:\s*var\(--focus-ring\);/s);
  });
});

describe("Body toolbar action buttons", () => {
  it("uses standard IconButton primitive with sm size", () => {
    const component = fileSystem.readFileSync(
      "src/features/requests/detail/BodyViewer.tsx",
      "utf8",
    );
    expect(component).toMatch(/<IconButton\b[^>]*size="sm"/s);
    expect(css).not.toMatch(/\.bodyActions\s*>\s*button/);
  });
});
