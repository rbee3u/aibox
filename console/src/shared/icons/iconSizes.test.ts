import { describe, expect, it } from "vitest";

import { iconSize } from "@/shared/icons/iconSizes";

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

function componentSources(directory: string): string[] {
  return fileSystem.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return componentSources(path);
    return entry.name.endsWith(".tsx") ? [path] : [];
  });
}

describe("Console icon sizes", () => {
  it("keeps the ladder ordered with no two steps within a pixel of each other", () => {
    const steps = Object.values(iconSize);
    for (const [index, step] of steps.entries())
      if (index > 0) expect(step).toBeGreaterThanOrEqual(steps[index - 1] + 2);
  });

  it("keeps one stroke weight beyond the emphasis a selection mark needs", () => {
    const weights = new Set(
      componentSources("src").flatMap((path) =>
        [...fileSystem.readFileSync(path, "utf8").matchAll(/strokeWidth=\{([\d.]+)\}/g)].map(
          (match) => match[1],
        ),
      ),
    );
    expect([...weights].sort()).toEqual(["3"]);
  });

  it("leaves no icon sizing itself outside the shared ladder", () => {
    const offenders = componentSources("src")
      .flatMap((path) => {
        const source = fileSystem.readFileSync(path, "utf8");
        return [...source.matchAll(/size=\{\d+\}/g)].map((match) => `${path}: ${match[0]}`);
      })
      .sort();
    expect(offenders).toEqual([]);
  });
});
