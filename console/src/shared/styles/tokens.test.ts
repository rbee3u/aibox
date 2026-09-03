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
  "control-rest",
  "action-primary-soft-ink",
  "action-primary-soft-surface",
  "action-primary-soft-line",
  "action-primary-soft-hover-ink",
  "action-primary-soft-hover-surface",
  "action-primary-soft-hover-line",
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
  "accent-contrast",
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
  "viz-request",
  "viz-wait",
  "viz-model",
  "viz-finalize",
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

  it("keeps quiet PrimarySoft readable and independent from selection", () => {
    for (const tokens of [light, dark]) {
      expect(tokens.get("action-primary-soft-surface")).toBe("transparent");
      expect(tokens.get("action-primary-soft-line")).toContain("color-mix");
      expect(tokens.get("action-primary-soft-hover-surface")).not.toBe(
        tokens.get("surface-selected"),
      );
      expect(
        contrastRatio(
          resolveToken(tokens, "action-primary-soft-ink"),
          resolveToken(tokens, "surface"),
        ),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(
          resolveToken(tokens, "action-primary-soft-hover-ink"),
          resolveToken(tokens, "action-primary-soft-hover-surface"),
        ),
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

  it("keeps the role-based typography hierarchy centralized", () => {
    expect(light.get("text-page-title")).toBe("18px");
    expect(light.get("text-panel-title")).toBe("var(--text-md)");
    expect(light.get("text-section-title")).toBe("var(--text-sm)");
    expect(light.get("text-row-title")).toBe("var(--text-sm)");
    expect(light.get("text-meta")).toBe("var(--text-xs)");
    expect(light.get("line-height-page-title")).toBe("var(--line-height-md)");
    expect(light.get("catalog-row-primary-size")).toBe("var(--text-row-title)");
    expect(light.get("catalog-row-secondary-size")).toBe("var(--text-meta)");
  });

  it("keeps the catalog Tenant/Agent filter toolbar rhythm centralized", () => {
    expect(light.get("catalog-filter-control-max-width")).toBe("112px");
    expect(light.get("catalog-toolbar-filters-gap")).toBe("8px");
    expect(light.get("catalog-toolbar-cluster-gap")).toBe("14px");
  });

  it("keeps neutral hover distinct from accent-owned selection in both themes", () => {
    expect(light.get("surface-hover")).toBe("#eef1f5");
    expect(light.get("surface-selected")).toBe("#eceaff");
    expect(dark.get("surface-hover")).toBe("#272f3d");
    expect(dark.get("surface-selected")).toBe("#292845");
    expect(light.get("surface-hover")).not.toBe(light.get("surface-selected"));
    expect(dark.get("surface-hover")).not.toBe(dark.get("surface-selected"));
  });

  it("keeps disabled Primary neutral instead of resembling accent selection", () => {
    expect(light.get("control-disabled-primary-ink")).toBe("var(--control-disabled-ink)");
    expect(light.get("control-disabled-primary-surface")).toBe("var(--control-disabled-surface)");
  });

  it("keeps hover, control rest, and accent selection roles distinct", () => {
    expect(light.get("control-rest")).toBe("#f4f6f9");
    expect(dark.get("control-rest")).toBe("#222833");
    expect(dark.get("surface-selected")).toBe("#292845");
    for (const tokens of [light, dark]) {
      expect(tokens.get("surface-hover")).not.toBe(tokens.get("control-rest"));
      expect(tokens.get("surface-hover")).not.toBe(tokens.get("surface-selected"));
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

function resolveToken(
  tokens: Map<string, string>,
  token: string,
  seen = new Set<string>(),
): string {
  if (seen.has(token)) throw new Error(`Circular token alias: ${token}`);
  const value = tokens.get(token);
  if (!value) throw new Error(`Missing token: ${token}`);
  const nextSeen = new Set(seen).add(token);
  return value.replace(/var\(--([\w-]+)\)/g, (_, alias: string) =>
    resolveToken(tokens, alias, nextSeen),
  );
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
