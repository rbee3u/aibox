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
const css = fileSystem.readFileSync("src/styles.css", "utf8");

const themeTokens = [
  "bg-canvas",
  "bg-shell",
  "surface",
  "surface-raised",
  "surface-inset",
  "surface-hover",
  "surface-selected",
  "line",
  "line-soft",
  "line-strong",
  "ink",
  "ink-secondary",
  "muted",
  "faint",
  "accent",
  "accent-strong",
  "accent-soft",
  "accent-subtle",
  "focus",
  "danger",
  "danger-strong",
  "danger-soft",
  "danger-line",
  "success",
  "success-soft",
  "warning",
  "warning-soft",
  "warning-line",
  "info-line",
  "code-bg",
  "code-border",
  "code-text",
  "code-muted",
  "code-guide",
  "syntax-key",
  "syntax-string",
  "syntax-number",
  "syntax-boolean",
  "shadow-sm",
  "shadow-md",
] as const;

describe("Console CSS theme tokens", () => {
  const light = declarations(":root");
  const dark = declarations(':root[data-resolved-theme="dark"]');

  it("keeps one complete semantic palette for each resolved theme", () => {
    for (const token of themeTokens) {
      expect(light.get(token), `light --${token}`).toBeTruthy();
      expect(dark.get(token), `dark --${token}`).toBeTruthy();
    }
    expect(css).not.toContain("--aibox-");
  });

  it("keeps primary interface text at WCAG AA contrast in both themes", () => {
    for (const tokens of [light, dark]) {
      const surface = tokens.get("surface")!;
      for (const foreground of [
        "ink",
        "ink-secondary",
        "muted",
        "faint",
        "accent",
        "danger",
        "success",
        "warning",
      ]) {
        expect(contrastRatio(tokens.get(foreground)!, surface), foreground).toBeGreaterThanOrEqual(
          4.5,
        );
      }
    }
  });
});

function declarations(selector: string): Map<string, string> {
  const start = css.search(new RegExp(`${escapeRegExp(selector)}\\s*\\{`));
  if (start < 0) throw new Error(`Missing CSS selector: ${selector}`);
  const bodyStart = css.indexOf("{", start) + 1;
  const body = css.slice(bodyStart, css.indexOf("}", bodyStart));
  return new Map(
    [...body.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((match) => [match[1], match[2].trim()]),
  );
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function contrastRatio(left: string, right: string): number {
  const [lighter, darker] = [luminance(left), luminance(right)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(color: string): number {
  const channels = color
    .slice(1)
    .match(/.{2}/g)!
    .map((value) => Number.parseInt(value, 16) / 255)
    .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}
