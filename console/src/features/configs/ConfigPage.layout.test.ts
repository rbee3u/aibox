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
const css = fileSystem.readFileSync("src/features/configs/ConfigPage.module.css", "utf8");

describe("config editor native content reminder", () => {
  it("uses muted metadata color instead of warning", () => {
    expect(css).toMatch(/\.editorNotice\s*\{[^}]*color:\s*var\(--muted\)/s);
    expect(css).not.toMatch(/\.editorNotice\s*\{[^}]*color:\s*var\(--warning\)/s);
  });
});

describe("visual field presence markers", () => {
  it("pins required and Optional marks to a shared trailing column", () => {
    expect(css).toMatch(
      /\.visualFieldMeta\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+24px\s+minmax\(5\.5em,\s*auto\)/s,
    );
    expect(css).toMatch(
      /\.visualFieldMeta\s*>\s*\.visualInclude\s*\{[^}]*grid-column:\s*2\s*\/\s*4/s,
    );
    expect(css).toMatch(/\.requiredMarker\s*\{[^}]*width:\s*24px[^}]*height:\s*24px/s);
    expect(css).toMatch(
      /\.visualFieldMeta\s*>\s*\.visualInclude\s*>\s*span:first-of-type,\s*\.proxyToggle\s*>\s*span:first-of-type\s*\{[^}]*width:\s*16px[^}]*height:\s*16px/s,
    );
    expect(css).toMatch(
      /\.visualFieldMeta\s*>\s*\.visualInclude\s+input:checked\s*\+\s*span:first-of-type::after,\s*\.proxyToggle\s+input:checked\s*\+\s*span:first-of-type::after\s*\{[^}]*top:\s*1px[^}]*left:\s*4px[^}]*width:\s*5px[^}]*height:\s*9px/s,
    );
  });
});
