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
const css = fileSystem.readFileSync("src/features/sessions/SessionPage.module.css", "utf8");
const component = fileSystem.readFileSync(
  "src/features/sessions/detail/SessionCopyValue.tsx",
  "utf8",
);

describe("Session detail copy controls", () => {
  it("takes the inline hit area from the primitive's own step", () => {
    expect(component).toMatch(/<IconButton\b[^>]*size="sm"/s);
  });

  /*
   * Sizing or restyling the control from this stylesheet is what once dropped
   * it to 24px on touch and later left it at 0.3 opacity while Overview and
   * Tenants drew theirs at rest: a feature stylesheet loads after the
   * primitive's, so an override here silently outranks it. The control now
   * has no feature-side class at all.
   */
  it("leaves the control's box and rest state to the primitive", () => {
    const control = /<IconButton\b([^>]*)>/s.exec(component);
    expect(control).not.toBeNull();
    expect(control![1]).not.toMatch(/className/);
    expect(css).not.toMatch(/\.sessionCopyAction\b/);
  });

  /*
   * A path is read for its tail: the Transcript path ends in the file name.
   * Truncating it left the copy control as the only way to read the value.
   */
  it("wraps a long value instead of truncating it", () => {
    const rule = /\.sessionCopyValue code\s*\{([^}]*)\}/s.exec(css);
    expect(rule).not.toBeNull();
    expect(rule![1]).toMatch(/overflow-wrap:\s*anywhere/);
    expect(rule![1]).not.toMatch(/nowrap|text-overflow/);
  });
});

describe("Session detail fact grids", () => {
  /*
   * A line-coloured container showing through a gap paints the unfilled slot
   * of an odd-count grid as a tile in the divider colour. Dividers are each
   * cell's own edges instead, so that slot is plain surface.
   */
  it("draws dividers as cell edges, not as a gap over a line-coloured fill", () => {
    const rule = /\.sessionDetailsGrid,\s*\.sessionDiagnosticsGrid\s*\{([^}]*)\}/s.exec(css);
    expect(rule).not.toBeNull();
    expect(rule![1]).toMatch(/background:\s*var\(--surface\)/);
    expect(rule![1]).not.toMatch(/gap\s*:/);
    expect(css).toMatch(/\.sessionDetailsGrid > div:nth-child\(n \+ 3\)[^{]*\{[^}]*border-top/s);
    expect(css).toMatch(/\.sessionDetailsGrid > div:nth-child\(2n\)[^{]*\{[^}]*border-left/s);
  });
});

describe("Session detail header", () => {
  it("never shrinks the action group below its controls", () => {
    expect(css).toMatch(/\.sessionDetailActions\s*\{[^}]*flex-shrink:\s*0/s);
  });
});

describe("Conversation navigation rail", () => {
  /*
   * The rail is the grid's first column. Without this the reading auto-places
   * into that 48px column whenever the rail has nothing to render — every open
   * until the first user message arrives, and any Session without one.
   */
  it("pins the reading to the second column", () => {
    expect(css).toMatch(/\.sessionConversationMain\s*\{[^}]*grid-column:\s*2/s);
  });

  it("runs the connector from the first stop's center to the last's", () => {
    const items = /\.sessionConversationRail \.sessionConversationNavItems\s*\{([^}]*)\}/s.exec(
      css,
    );
    const stop = /\.sessionConversationRail button\s*\{([^}]*)\}/s.exec(css);
    const line =
      /\.sessionConversationRail \.sessionConversationNavItems::before\s*\{([^}]*)\}/s.exec(css);
    expect(items).not.toBeNull();
    expect(stop).not.toBeNull();
    expect(line).not.toBeNull();
    const padding = Number(/padding:\s*(\d+)px 0/.exec(items![1])![1]);
    const height = Number(/height:\s*(\d+)px/.exec(stop![1])![1]);
    const center = padding + height / 2;
    expect(line![1]).toMatch(new RegExp(`top:\\s*${center}px`));
    expect(line![1]).toMatch(new RegExp(`bottom:\\s*${center}px`));
    expect(items![1]).not.toMatch(/align-self|flex:\s*1/);
    expect(css).toMatch(/\.sessionConversationRail\s*\{[^}]*align-items:\s*flex-start/s);
  });
});
