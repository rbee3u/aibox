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

type Rule = { selector: string; body: string };

function rules(source: string): Rule[] {
  return [
    ...source.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/^([^{}\n@][^{}]*?)\s*\{([^}]*?)\}/gms),
  ].map((match) => ({ selector: match[1], body: match[2] }));
}

/** The pressed selector a resting one implies, matching how the sheets derive it. */
function pressedForm(selector: string): string {
  return selector
    .replace(/:hover|:focus-within/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^(\S.*?[\w\])])((?::not\([^)]*\))*)$/, "$1:active$2");
}

const sheets = stylesheets("src").map((path) => ({
  path,
  source: fileSystem.readFileSync(path, "utf8"),
}));

describe("Console interaction states", () => {
  it("finds a stylesheet to read", () => {
    expect(sheets.length).toBeGreaterThan(20);
  });

  it("answers a press wherever a surface already answers a pointer at rest", () => {
    const unanswered = sheets.flatMap(({ path, source }) => {
      // A sheet may answer per selector, or once with a layer over a shared
      // base that every variant in the sheet already composes from.
      if (/background-image:\s*linear-gradient\(var\(--surface-pressed\)/.test(source)) return [];
      const pressed = new Set(
        rules(source)
          .filter(({ body }) => /background:\s*var\(--surface-pressed\)/.test(body))
          .flatMap(({ selector }) =>
            selector.split(",").map((part) => part.replace(/\s+/g, " ").trim()),
          ),
      );
      return rules(source)
        .filter(({ body }) => /background:\s*var\(--surface-(hover|selected)\)/.test(body))
        .flatMap(({ selector }) => selector.split(",").map(pressedForm))
        .filter((selector) => !pressed.has(selector))
        .map((selector) => `${path}: ${selector}`);
    });
    expect([...new Set(unanswered)].sort()).toEqual([]);
  });

  /*
   * A state that lasts has to be drawn from the ladder that hover and press
   * come from, or it cannot be ranked against them. The current module was
   * filled with an accent tint measured against nothing, and it landed below
   * the hover an idle module gets from a passing pointer.
   */
  it("fills a lasting state from the same ladder a passing one comes from", () => {
    const offenders = sheets.flatMap(({ path, source }) =>
      rules(source)
        .filter(({ selector }) => /\[aria-current/.test(selector))
        .flatMap(({ selector, body }) =>
          [...body.matchAll(/background(?:-color)?:\s*([^;]+)/g)]
            .map(([, value]) => value.trim())
            .filter((value) => !/^(?:var\(--surface-|transparent$|none$)/.test(value))
            .map((value) => `${path}: ${selector.trim()} -> ${value}`),
        ),
    );
    expect(offenders.sort()).toEqual([]);
  });

  /*
   * Whether a given surface eases its fill depends on which element the
   * component puts the class on, which no reading of the stylesheets can
   * settle. Rather than approximate it with a rule that needs an exemption
   * list, this pins the discipline that is decidable: nothing invents a time.
   */
  it("times every transition from a duration token", () => {
    const raw = sheets.flatMap(({ path, source }) =>
      [...source.matchAll(/transition:\s*([^;]+);/g)]
        .filter(([, value]) => /\d+m?s\b/.test(value))
        .map(([, value]) => `${path}: ${value.replace(/\s+/g, " ")}`),
    );
    expect(raw.sort()).toEqual([]);
  });

  it("keeps the press instant and the reduced-motion opt-out in one place", () => {
    const tokens = sheets.find(({ path }) => path.endsWith("shared/styles/tokens.css"))!;
    expect(tokens.source).toMatch(/\*:active \{\n {2}--transition-fast: 0s;\n\}/);
    const optOuts = sheets.filter(({ source }) => source.includes("prefers-reduced-motion"));
    expect(optOuts.map(({ path }) => path)).toEqual([tokens.path]);
  });

  it("draws every focus ring from the shared treatment and distance", () => {
    const strays = sheets.flatMap(({ path, source }) =>
      [...source.matchAll(/(outline(?:-offset)?):\s*([^;]+);/g)]
        .filter(([, property, value]) =>
          property === "outline"
            ? !/^(var\(--focus-ring\)|none|0)$/.test(value.trim())
            : !/^(var\(--focus-offset\)|calc\(var\(--focus-offset\) \* -1\))$/.test(value.trim()),
        )
        .map(([, property, value]) => `${path}: ${property}: ${value.trim()}`),
    );
    expect(strays.sort()).toEqual([]);
  });

  it("reserves the ring for keyboard focus rather than every click", () => {
    const bare = sheets.flatMap(({ path, source }) =>
      rules(source)
        .filter(({ selector }) => /:focus(?![-\w])/.test(selector))
        .map(({ selector }) => `${path}: ${selector.replace(/\s+/g, " ").trim()}`),
    );
    expect(bare.sort()).toEqual(["src/app/App.module.css: .skipLink:focus"]);
  });
});
