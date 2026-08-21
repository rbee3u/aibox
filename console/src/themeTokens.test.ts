import { describe, expect, it, vi } from "vitest";
import { consoleThemeTokens, resolveTheme } from "./themeTokens";

describe("Console theme tokens", () => {
  it("keeps a complete semantic token set for both themes", () => {
    expect(Object.keys(consoleThemeTokens.light).sort()).toEqual(
      Object.keys(consoleThemeTokens.dark).sort(),
    );
    for (const tokens of Object.values(consoleThemeTokens)) {
      for (const value of Object.values(tokens)) expect(value).not.toBe("");
    }
  });

  it("resolves system preference without changing explicit themes", () => {
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));
    expect(resolveTheme("system")).toBe("dark");
  });

  it("keeps primary interface text at WCAG AA contrast in both themes", () => {
    for (const tokens of Object.values(consoleThemeTokens)) {
      for (const foreground of [
        tokens.ink,
        tokens.inkSecondary,
        tokens.muted,
        tokens.faint,
        tokens.accent,
        tokens.danger,
        tokens.success,
        tokens.warning,
      ]) {
        expect(contrastRatio(foreground, tokens.surface)).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});

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
