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

describe("file stack", () => {
  it("scrolls as one region with content-sized files and sticky file headers", () => {
    expect(css).toMatch(/\.configFileStack\s*\{[^}]*overflow:\s*auto/s);
    expect(css).toMatch(/\.configFileSection\s*\{[^}]*flex:\s*0 0 auto/s);
    expect(css).toMatch(/\.editorTools\s*\{[^}]*position:\s*sticky/s);
    expect(css).toMatch(/\.cm-scroller\)\s*\{[^}]*overflow:\s*visible/s);
    expect(css).not.toMatch(/\.cm-scroller\)\s*\{[^}]*height:\s*100%/s);
  });
});

describe("editor chrome", () => {
  it("routes the library's selection, search, panel, and tooltip colours through tokens", () => {
    for (const part of [
      "cm-selectionBackground",
      "cm-searchMatch",
      "cm-panels",
      "cm-textfield",
      "cm-button",
      "cm-tooltip",
      "cm-matchingBracket",
      "cm-cursor",
    ]) {
      expect(css, part).toMatch(new RegExp(`\\.${part}\\)[^{]*\\{[^}]*var\\(--`, "s"));
    }
  });
});
