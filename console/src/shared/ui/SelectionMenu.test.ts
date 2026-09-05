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
const css = fileSystem.readFileSync("src/shared/ui/SelectionMenu.module.css", "utf8");

describe("SelectionMenu visual states", () => {
  it("keeps hover neutral and reserves accent fill for the open state", () => {
    expect(css).toMatch(
      /\.selectionTrigger:hover:not\(:disabled\):not\(\[aria-expanded="true"\]\)\s*\{[^}]*background:\s*var\(--surface-hover\)/s,
    );
    expect(css).toMatch(
      /\.selectionTrigger\[aria-expanded="true"\]:not\(:disabled\)\s*\{[^}]*background:\s*var\(--surface-selected\)/s,
    );
    expect(css).toMatch(
      /\.selectionTrigger:hover:not\(:disabled\):not\(\[aria-expanded="true"\]\)\s*\{[^}]*color:\s*var\(--ink-secondary\)/s,
    );
    expect(css).toMatch(
      /\.selectionOption:hover\s*\{[^}]*color:\s*var\(--ink-secondary\)[^}]*background:\s*var\(--surface-hover\)/s,
    );
    expect(css).toMatch(
      /\.selectionOption:has\(input:checked\)\s*\{[^}]*color:\s*var\(--ink\)[^}]*background:\s*var\(--surface-selected\)/s,
    );
  });

  it("keeps overlay menus raised and field triggers on the form surface", () => {
    expect(css).toMatch(
      /\.selectionMenu\s*\{[^}]*background:\s*var\(--surface-raised\)[^}]*box-shadow:\s*var\(--shadow-lg\)/s,
    );
    expect(css).toMatch(
      /\.selectionField \.selectionTrigger\s*\{[^}]*border-color:\s*var\(--line-control\)[^}]*background:\s*var\(--surface\)/s,
    );
    expect(css).toMatch(
      /\.selectionField \.selectionTrigger:hover:not\(:disabled\):not\(\[aria-expanded="true"\]\)\s*\{[^}]*background:\s*var\(--surface-hover\)/s,
    );
  });
});
