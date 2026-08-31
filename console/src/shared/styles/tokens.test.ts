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
const css = fileSystem.readFileSync("src/shared/styles/tokens.css", "utf8");

const themeTokens = [
  "bg-canvas",
  "bg-shell",
  "surface",
  "surface-raised",
  "surface-inset",
  "surface-hover",
  "surface-selected",
  "surface-row-hover",
  "control-rest",
  "control-danger-rest",
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
  "component-update-action",
  "component-update-action-hover",
  "component-update-action-ink",
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

  it("keeps the Component Update action at WCAG AA contrast in both themes", () => {
    for (const tokens of [light, dark]) {
      const foreground = tokens.get("component-update-action-ink")!;
      expect(
        contrastRatio(foreground, tokens.get("component-update-action")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(foreground, tokens.get("component-update-action-hover")!),
      ).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps Component Install, warning, and focus treatments distinguishable", () => {
    for (const tokens of [light, dark]) {
      expect(
        contrastRatio(tokens.get("accent")!, tokens.get("accent-soft")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(tokens.get("accent")!, tokens.get("surface-selected")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(tokens.get("danger-strong")!, tokens.get("danger-soft")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(tokens.get("warning")!, tokens.get("warning-soft")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(tokens.get("focus")!, tokens.get("surface")!)).toBeGreaterThanOrEqual(3);
    }
  });

  it("keeps the compact Console density contract centralized", () => {
    expect(light.get("control-compact")).toBe("30px");
    expect(light.get("control-sm")).toBe("32px");
    expect(light.get("control-md")).toBe("36px");
    expect(light.get("toolbar-height")).toBe("44px");
    expect(light.get("row-height")).toBe("46px");
    expect(light.get("row-height-roomy")).toBe("54px");
    expect(light.get("radius-sm")).toBe("5px");
    expect(light.get("radius-md")).toBe("6px");
  });

  it("keeps the catalog Tenant/Agent filter toolbar rhythm centralized", () => {
    expect(light.get("catalog-filter-control-max-width")).toBe("112px");
    expect(light.get("catalog-toolbar-filters-gap")).toBe("8px");
    expect(light.get("catalog-toolbar-cluster-gap")).toBe("14px");
  });

  it("keeps chrome hover between list wash and control float in both themes", () => {
    expect(light.get("surface-hover")).toBe("#f1f0ff");
    expect(light.get("surface-selected")).toBe("#eceaff");
    expect(dark.get("surface-hover")).toBe("#25253e");
    expect(dark.get("surface-selected")).toBe("#292845");
    expect(light.get("surface-hover")).not.toBe(light.get("surface-selected"));
    expect(dark.get("surface-hover")).not.toBe(dark.get("surface-selected"));
  });

  it("keeps list wash distinct from control rest and control float", () => {
    expect(light.get("surface-row-hover")).toBe("var(--accent-subtle)");
    expect(light.get("accent-subtle")).toBe("#f8f7ff");
    expect(dark.get("surface-row-hover")).toBe("#1f1e32");
    expect(dark.get("control-rest")).toBe("#242338");
    expect(dark.get("surface-selected")).toBe("#292845");
    for (const tokens of [light, dark]) {
      // List hover and selected share --surface-row-hover; float uses --surface-selected.
      expect(tokens.get("surface-row-hover")).not.toBe(tokens.get("control-rest"));
      expect(tokens.get("surface-row-hover")).not.toBe(tokens.get("surface-selected"));
      expect(tokens.get("control-rest")).not.toBe(tokens.get("surface-selected"));
    }
  });

  it("keeps the escaping-surface stacking order centralized and ordered", () => {
    const order = [
      "layer-inline-banner",
      "layer-dropdown",
      "layer-scrim",
      "layer-sidebar",
      "layer-dock",
      "layer-row-menu",
      "layer-notification",
      "layer-tooltip",
      "layer-portal-menu",
    ];
    const values = order.map((token) => {
      const value = light.get(token);
      expect(value, token).toBeDefined();
      return Number(value);
    });
    for (const [index, value] of values.entries()) {
      expect(Number.isInteger(value), order[index]).toBe(true);
      if (index > 0)
        expect(value, `${order[index]} above ${order[index - 1]}`).toBeGreaterThan(
          values[index - 1],
        );
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
