import { describe, expect, it } from "vitest";

const fileSystem = (
  globalThis as typeof globalThis & {
    process: {
      getBuiltinModule(name: "fs"): {
        readFileSync(path: string, encoding: "utf8"): string;
        readdirSync(
          path: string,
          options: { withFileTypes: true },
        ): { name: string; isDirectory(): boolean }[];
      };
    };
  }
).process.getBuiltinModule("fs");

function stylesheets(directory: string): string[] {
  return fileSystem.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return stylesheets(path);
    return entry.name.endsWith(".css") ? [path] : [];
  });
}

const sheets = stylesheets("src").map((path) => ({
  path,
  source: fileSystem.readFileSync(path, "utf8"),
}));

describe("Console type ladder", () => {
  it("finds a stylesheet to read", () => {
    expect(sheets.length).toBeGreaterThan(20);
  });

  /*
   * A size written inline is a size nobody can compare against its siblings.
   * Every one that existed had drifted: two promoted a single value above the
   * strip it belonged to, and one sat below the readable floor.
   */
  it("leaves no type size written outside the ladder", () => {
    const offenders = sheets.flatMap(({ path, source }) =>
      [...source.matchAll(/(?:^|[;{\s])(font-size|font):\s*([^;}]+)/g)]
        .filter(([, , value]) => /\b\d+(?:\.\d+)?(?:px|rem|em)\b/.test(value))
        .map(([, property, value]) => `${path}: ${property}: ${value.trim()}`),
    );
    expect(offenders.sort()).toEqual([]);
  });

  it("keeps a panel title step that only titles may spend", () => {
    const borrowed = sheets.flatMap(({ path, source }) =>
      [...source.matchAll(/([^{}]*)\{[^}]*var\(--text-(?:page|panel)-title\)[^}]*\}/g)]
        .flatMap(([, selector]) => selector.split(","))
        .map((selector) => selector.replace(/\s+/g, " ").trim())
        .filter((selector) => /value|metric|count|badge/i.test(selector))
        .map((selector) => `${path}: ${selector}`),
    );
    expect(borrowed.sort()).toEqual([]);
  });
});
