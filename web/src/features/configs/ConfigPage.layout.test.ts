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
const css = [
  "src/features/configs/catalog/ConfigCatalogPane.module.css",
  "src/features/configs/shared.module.css",
  "src/features/configs/mutation/ConfigDialogs.module.css",
  "src/features/configs/editor/ConfigDetailPane.module.css",
  "src/features/configs/ConfigDriftBadge.module.css",
  "src/features/configs/editor/ConfigFilePane.module.css",
  "src/features/configs/editor/editor.module.css",
  "src/features/configs/editor/VisualConfigOptions.module.css",
  "src/features/configs/editor/ConfigDifferences.module.css",
]
  .map((path) => fileSystem.readFileSync(path, "utf8"))
  .join("\n");

describe("config editor native content reminder", () => {
  it("uses muted metadata color instead of warning", () => {
    expect(css).toMatch(/\.editorNotice\s*\{[^}]*color:\s*var\(--muted\)/s);
    expect(css).not.toMatch(/\.editorNotice\s*\{[^}]*color:\s*var\(--warning\)/s);
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
