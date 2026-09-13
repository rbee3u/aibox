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
   * Sizing the control from this stylesheet is what once dropped it to 24px on
   * touch: a feature stylesheet loads after the primitive's, so an override
   * here silently outranks the coarse-pointer floor rather than layering on it.
   */
  it("does not size the control from the feature stylesheet", () => {
    const rule = /\.sessionCopyAction\s*\{([^}]*)\}/s.exec(css);
    expect(rule).not.toBeNull();
    expect(rule![1]).not.toMatch(/(?:min-)?(?:width|height)\s*:/);
    expect(rule![1]).not.toMatch(/flex\s*:/);
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
