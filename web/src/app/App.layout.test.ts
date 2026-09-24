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
const css = fileSystem.readFileSync("src/app/App.module.css", "utf8");

function declarations(source: string, selector: string): string {
  const marker = `${selector} {`;
  const start = source.indexOf(marker);
  expect(start).toBeGreaterThanOrEqual(0);
  const end = source.indexOf("}", start);
  expect(end).toBeGreaterThan(start);
  return source.slice(start + marker.length, end);
}

function verticalPadding(rule: string): string {
  const match = /padding:\s*(\d+px)(?:\s+\d+px)?/.exec(rule);
  expect(match).not.toBeNull();
  return match![1];
}

describe("shell icon controls", () => {
  it("does not shrink Collapse sidebar and Open navigation below the 36px IconButton", () => {
    expect(css).not.toMatch(
      /\.collapseButton,\s*\.menuButton\s*\{[^}]*width:\s*30px;[^}]*height:\s*30px/s,
    );
    expect(css).toMatch(/\.collapseButton\s*\{[^}]*flex:\s*0 0 var\(--control-md\)/s);
  });

  it("uses one drawer scroll surface when the viewport is short", () => {
    expect(css).toContain("@media (max-width: 900px) and (max-height: 480px)");
    expect(css).toMatch(/\.sidebar\s*\{\s*display:\s*block;\s*overflow-y:\s*auto;\s*\}/s);
    expect(css).toMatch(
      /\.brand\s*\{[^}]*position:\s*sticky;[^}]*inset-block-start:\s*0;[^}]*background:\s*var\(--bg-shell\);[^}]*\}/s,
    );
    expect(css).toMatch(/\.moduleNav\s*\{\s*overflow-y:\s*visible;\s*\}/s);
  });
});

describe("brand lockup", () => {
  it("keeps the name intact while only the inline tagline can truncate", () => {
    const copy = declarations(css, ".brandCopy");
    const name = declarations(css, ".brand strong");
    const separator = declarations(css, ".brandSeparator");
    const tagline = declarations(css, ".brand small");

    expect(copy).toMatch(/display:\s*flex/);
    expect(copy).toMatch(/align-items:\s*baseline/);
    expect(copy).toMatch(/gap:\s*var\(--space-sm\)/);
    expect(name).toMatch(/flex:\s*0 0 auto/);
    expect(name).toMatch(/white-space:\s*nowrap/);
    expect(separator).toMatch(/color:\s*var\(--faint\)/);
    expect(separator).toMatch(/font-size:\s*var\(--text-xs\)/);
    expect(tagline).toMatch(/min-width:\s*0/);
    expect(tagline).toMatch(/flex:\s*1 1 auto/);
    expect(tagline).toMatch(/color:\s*var\(--faint\)/);
    expect(tagline).toMatch(/font-weight:\s*var\(--weight-regular\)/);
    expect(tagline).toMatch(/text-overflow:\s*ellipsis/);
    expect(tagline).toMatch(/white-space:\s*nowrap/);
  });
});

describe("primary module navigation", () => {
  it("uses one padding-driven target across expanded, collapsed, and drawer layouts", () => {
    const expanded = declarations(css, ".moduleNav a");
    const collapsed = declarations(css, ".collapsed .moduleNav a");
    const narrow = css.slice(css.indexOf("@media (max-width: 900px) {"));
    const drawer = declarations(narrow, ".collapsed .moduleNav a");
    const targets = [expanded, collapsed, drawer];

    expect(declarations(css, ".moduleNav")).toMatch(/gap:\s*var\(--space-xs\)/);
    expect(new Set(targets.map(verticalPadding)).size).toBe(1);

    for (const target of targets) {
      expect(target).not.toMatch(/(?:min-)?height\s*:/);
    }
  });
});
