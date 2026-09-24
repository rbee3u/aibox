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

  it("provides hover, active, and focus micro-interactions with rounded corners", () => {
    expect(css).toMatch(/\.jsonToggle:hover\s*\{[^}]*background:\s*var\(--surface-hover\);/s);
    expect(css).toMatch(/\.jsonToggle:active\s*\{[^}]*background:\s*var\(--surface-pressed\);/s);
    expect(css).toMatch(/\.jsonToggle:focus-visible\s*\{[^}]*outline:\s*var\(--focus-ring\);/s);
    expect(css).toMatch(/\.jsonCopy:hover\s*\{[^}]*background:\s*var\(--surface-hover\);/s);
    expect(css).toMatch(/\.jsonCopy:active\s*\{[^}]*background:\s*var\(--surface-pressed\);/s);
    expect(css).toMatch(/\.jsonStringToggle:hover\s*\{[^}]*background:\s*var\(--surface-hover\);/s);
    expect(css).toMatch(
      /\.jsonStringToggle:active\s*\{[^}]*background:\s*var\(--surface-pressed\);/s,
    );
  });
});
