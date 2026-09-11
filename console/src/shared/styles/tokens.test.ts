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
  "action-primary-ink",
  "action-primary-surface",
  "action-primary-hover-surface",
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
  "success-line",
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
  "shadow-lg",
] as const;

describe("Console CSS theme tokens", () => {
  const light = declarations(":root");
  const dark = declarations(':root[data-resolved-theme="dark"]');
  const themes = [
    ["light", light],
    ["dark", dark],
  ] as const;

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

  it("keeps each text role quieter than the one above it in both themes", () => {
    for (const [theme, tokens] of themes) {
      const ladder = ["ink", "ink-secondary", "muted", "faint"];
      const prominence = ladder.map((role) =>
        contrastRatio(tokens.get(role)!, tokens.get("surface")!),
      );
      for (const [index, step] of prominence.entries())
        if (index > 0)
          expect(step, `${theme} ${ladder[index]} quieter than ${ladder[index - 1]}`).toBeLessThan(
            prominence[index - 1],
          );
    }
  });

  it("derives the focus ring from the accent instead of a fourth indigo", () => {
    for (const [theme, tokens] of themes)
      expect(tokens.get("focus"), `${theme} focus`).toBe("var(--accent)");
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
      expect(
        contrastRatio(
          resolveToken(tokens, "action-primary-ink"),
          resolveToken(tokens, "action-primary-surface"),
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
        contrastRatio(
          tokens.get("accent")!,
          flattenOver(tokens, "surface-selected", tokens.get("surface")!),
        ),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(tokens.get("danger-strong")!, tokens.get("danger-soft")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(tokens.get("warning")!, tokens.get("warning-soft")!),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(resolveToken(tokens, "focus"), tokens.get("surface")!),
      ).toBeGreaterThanOrEqual(3);
    }
  });

  it("keeps the compact Console density contract centralized", () => {
    expect(light.get("control-compact")).toBe("30px");
    expect(light.get("control-xs")).toBe("24px");
    expect(light.get("control-sm")).toBe("32px");
    expect(light.get("control-md")).toBe("36px");
    expect(light.get("toolbar-height")).toBe("44px");
    expect(light.get("row-height")).toBe("46px");
    expect(light.get("row-height-roomy")).toBe("54px");
    expect(light.get("radius-sm")).toBe("5px");
    expect(light.get("radius-md")).toBe("6px");
  });

  it("keeps the spacing scale on a two-pixel base and ordered", () => {
    const ladder = ["2xs", "xs", "sm", "md", "lg", "xl", "2xl", "3xl"];
    const steps = ladder.map((step) => {
      const value = light.get(`space-${step}`);
      expect(value, `space-${step}`).toMatch(/^\d+px$/);
      return Number.parseInt(value!, 10);
    });
    for (const [index, step] of steps.entries()) {
      expect(step % 2, `space-${ladder[index]} on the two-pixel base`).toBe(0);
      if (index > 0)
        expect(step, `space-${ladder[index]} above space-${ladder[index - 1]}`).toBeGreaterThan(
          steps[index - 1],
        );
    }
  });

  it("keeps a section title far closer to its content than to the next section", () => {
    const step = (role: string) => {
      const alias = light.get(role)?.match(/^var\(--(space-[\w-]+)\)$/);
      expect(alias, role).not.toBeNull();
      return Number.parseInt(light.get(alias![1])!, 10);
    };
    expect(step("space-section")).toBeGreaterThanOrEqual(step("space-section-title") * 3);
  });

  it("keeps the content measure ladder centralized and ordered", () => {
    const ladder = ["measure-compact", "measure-text", "measure-data"];
    const widths = ladder.map((token) => {
      const value = light.get(token);
      expect(value, token).toMatch(/^\d+px$/);
      return Number.parseInt(value!, 10);
    });
    for (const [index, width] of widths.entries())
      if (index > 0)
        expect(width, `${ladder[index]} wider than ${ladder[index - 1]}`).toBeGreaterThan(
          widths[index - 1],
        );
  });

  it("keeps the role-based typography hierarchy centralized", () => {
    expect(light.get("text-page-title")).toBe("var(--text-lg)");
    expect(light.get("text-panel-title")).toBe("var(--text-lg)");
    expect(light.get("text-section-title")).toBe("var(--text-xs)");
    expect(light.get("text-row-title")).toBe("var(--text-sm)");
    expect(light.get("text-meta")).toBe("var(--text-xs)");
    expect(light.get("line-height-page-title")).toBe("var(--line-height-md)");
    expect(light.get("catalog-row-primary-size")).toBe("var(--text-row-title)");
    expect(light.get("catalog-row-secondary-size")).toBe("var(--text-meta)");
  });

  it("keeps every type role on a step of the size scale", () => {
    const scale = new Map<string, number>(
      ["xs", "sm", "md", "lg"].map((step) => {
        const value = light.get(`text-${step}`);
        expect(value, `text-${step}`).toMatch(/^\d+px$/);
        return [`var(--text-${step})`, Number.parseInt(value!, 10)];
      }),
    );
    const roles = ["page-title", "panel-title", "section-title", "row-title", "meta"];
    for (const role of roles) expect(scale.has(light.get(`text-${role}`)!), role).toBe(true);
    const px = (role: string) => scale.get(light.get(`text-${role}`)!)!;
    expect(px("page-title")).toBeGreaterThan(px("row-title"));
    expect(px("panel-title")).toBeGreaterThan(px("row-title"));
  });

  it("keeps a section title quieter than the rows it groups", () => {
    expect(light.get("text-section-title")).not.toBe(light.get("text-row-title"));
    expect(light.get("ink-section-title")).toBe("var(--muted)");
    expect(light.get("weight-row-title")).toBe("var(--weight-semibold)");
  });

  it("reserves bold for the page title so the narrow top bar outranks a panel", () => {
    expect(light.get("text-page-title")).toBe(light.get("text-panel-title"));
    expect(light.get("weight-page-title")).toBe("var(--weight-bold)");
    expect(light.get("weight-panel-title")).toBe("var(--weight-semibold)");
  });

  it("keeps the catalog Tenant/Agent filter toolbar rhythm centralized", () => {
    expect(light.get("catalog-filter-control-max-width")).toBe("112px");
    expect(light.get("catalog-toolbar-filters-gap")).toBe("8px");
    expect(light.get("catalog-toolbar-cluster-gap")).toBe("14px");
  });

  it("keeps chrome a perceptibly different material from the content it frames", () => {
    for (const [theme, tokens] of themes) {
      const ladder = ["bg-canvas", "bg-shell", "surface"];
      const fills = ladder.map((token) => tokens.get(token)!);
      for (const [index, fill] of fills.entries())
        if (index > 0) {
          const previous = fills[index - 1];
          expect(
            luminance(fill),
            `${theme} ${ladder[index]} above ${ladder[index - 1]}`,
          ).toBeGreaterThan(luminance(previous));
          expect(contrastRatio(fill, previous), `${theme} ${ladder[index]} step`).toBeGreaterThan(
            1.04,
          );
        }
      const inset = luminance(tokens.get("surface-inset")!);
      expect(inset, `${theme} inset recessed from content`).toBeLessThan(
        luminance(tokens.get("surface")!),
      );
      expect(inset, `${theme} inset shallower than chrome`).toBeGreaterThan(
        luminance(tokens.get("bg-shell")!),
      );
    }
  });

  it("carries elevation with fill in the dark theme and with shadow in the light one", () => {
    expect(light.get("surface-raised")).toBe(light.get("surface"));
    for (const level of ["shadow-sm", "shadow-md", "shadow-lg"])
      expect(light.get(level)!.match(/rgb\(/g), `light --${level} layers`).toHaveLength(2);
    expect(luminance(dark.get("surface-raised")!)).toBeGreaterThan(luminance(dark.get("surface")!));
  });

  it("keeps neutral hover distinct from selected chrome on every surface they sit on", () => {
    for (const [theme, tokens] of themes) {
      expect(tokens.get("surface-hover"), `${theme} hover`).toContain("var(--ink)");
      expect(tokens.get("surface-selected"), `${theme} selected`).toContain("var(--ink)");
      expect(tokens.get("surface-selected"), `${theme} selected`).not.toContain("accent");
      for (const base of ["surface", "bg-shell", "surface-inset"]) {
        const fill = tokens.get(base)!;
        const hover = contrastRatio(flattenOver(tokens, "surface-hover", fill), fill);
        const selected = contrastRatio(flattenOver(tokens, "surface-selected", fill), fill);
        expect(hover, `${theme} hover on ${base}`).toBeGreaterThan(1.05);
        expect(selected, `${theme} selected on ${base}`).toBeGreaterThan(hover);
      }
    }
  });

  it("keeps disabled Primary neutral instead of resembling accent selection", () => {
    expect(light.get("control-disabled-primary-ink")).toBe("var(--control-disabled-ink)");
    expect(light.get("control-disabled-primary-surface")).toBe("var(--control-disabled-surface)");
  });

  it("keeps overlay elevation stronger than resting chrome in both themes", () => {
    expect(dark.get("shadow-lg")).toBe("0 20px 52px rgb(0 0 0 / 0.38)");
    for (const [theme, tokens] of themes) {
      expect(tokens.get("shadow-lg"), `${theme} shadow-lg`).not.toBe(tokens.get("shadow-md"));
      expect(tokens.get("shadow-md"), `${theme} shadow-md`).not.toBe(tokens.get("shadow-sm"));
    }
  });

  it("keeps hover, control rest, and accent selection roles distinct", () => {
    expect(light.get("control-rest")).toBe("#f4f7fa");
    expect(dark.get("control-rest")).toBe("#222833");
    for (const [theme, tokens] of themes) {
      const surface = tokens.get("surface")!;
      const rest = tokens.get("control-rest")!;
      for (const role of ["surface-hover", "surface-selected"])
        expect(
          flattenOver(tokens, role, surface),
          `${theme} ${role} against control rest`,
        ).not.toBe(rest);
      expect(tokens.get("surface-hover")).not.toBe(tokens.get("surface-selected"));
    }
  });

  /*
   * On both backgrounds the ladder lands on, not just the content one. The
   * sidebar paints these over the shell, and checking only the surface let the
   * current module keep a fill fainter than a passing hover.
   */
  it("drives a press past every resting state it can land on", () => {
    for (const [theme, tokens] of themes) {
      for (const base of ["surface", "bg-shell"]) {
        const fill = tokens.get(base)!;
        const depth = (role: string) => contrastRatio(flattenOver(tokens, role, fill), fill);
        expect(depth("surface-pressed"), `${theme} press past selected on ${base}`).toBeGreaterThan(
          depth("surface-selected"),
        );
        expect(
          depth("surface-selected"),
          `${theme} selected past hover on ${base}`,
        ).toBeGreaterThan(depth("surface-hover"));
        expect(
          contrastRatio(tokens.get("ink")!, flattenOver(tokens, "surface-pressed", fill)),
          `${theme} ink on a pressed ${base}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("keeps the focus ring one treatment at one distance", () => {
    for (const [theme, tokens] of themes) {
      expect(tokens.get("focus-ring"), `${theme} ring`).toBe("2px solid var(--focus)");
      expect(tokens.get("focus-offset"), `${theme} offset`).toBe("2px");
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

/** Composites a translucent overlay token onto an opaque fill. */
function flattenOver(tokens: Map<string, string>, token: string, fill: string): string {
  const overlay = resolveToken(tokens, token).match(
    /^color-mix\(in srgb, (#[0-9a-f]{6}) (\d+)%, transparent\)$/,
  );
  if (!overlay) return resolveToken(tokens, token);
  const [ink, alpha] = [channels(overlay[1]), Number(overlay[2]) / 100];
  return `#${channels(fill)
    .map((base, index) =>
      Math.round(base + (ink[index] - base) * alpha)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function channels(color: string): number[] {
  return color
    .slice(1)
    .match(/.{2}/g)!
    .map((value) => Number.parseInt(value, 16));
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
