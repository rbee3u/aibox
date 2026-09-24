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
const css = fileSystem.readFileSync(
  "src/features/requests/detail/RequestDetail.module.css",
  "utf8",
);
const component = fileSystem.readFileSync("src/features/requests/detail/RequestDetail.tsx", "utf8");

describe("Request summary copy controls", () => {
  it("takes the inline hit area from the primitive's own step", () => {
    expect(component).toMatch(/<IconButton\b[^>]*size="sm"/s);
  });

  it("leaves the control's box and rest state to the primitive", () => {
    expect(css).not.toMatch(/\.copySession\b/);
  });
});

describe("Request timing waterfall", () => {
  it("uses substantial 10px track height and 5px minimum bar width", () => {
    expect(css).toMatch(/\.timelineTrack\s*\{[^}]*height:\s*10px;/s);
    expect(css).toMatch(/\.timelineBar\s*\{[^}]*width:\s*max\(5px,/s);
  });

  it("uses solid semantic tokens without 42% opacity washing", () => {
    expect(css).toMatch(/\.toneRequest\s*\{[^}]*background:\s*var\(--viz-request\);/s);
    expect(css).toMatch(/\.toneWait\s*\{[^}]*background:\s*var\(--viz-wait\);/s);
    expect(css).toMatch(/\.toneModel\s*\{[^}]*background:\s*var\(--viz-model\);/s);
    expect(css).toMatch(/\.toneFinalize\s*\{[^}]*background:\s*var\(--viz-finalize\);/s);
  });
});

describe("Request diagnostics presentation", () => {
  it("uses perimeter borders and rounded corners for diagnostic cards", () => {
    expect(css).toMatch(/\.diagnosticItem\s*\{[^}]*border:\s*1px solid var\(--line-content\);/s);
    expect(css).toMatch(/\.diagnosticItem\s*\{[^}]*border-left-width:\s*3px;/s);
    expect(css).toMatch(/\.diagnosticItem\s*\{[^}]*border-radius:\s*var\(--radius-sm\);/s);
  });

  it("uses natural body font and tone colors for message and count badges", () => {
    expect(css).toMatch(/\.diagnosticItem p\s*\{[^}]*font-family:\s*inherit;/s);
    expect(css).toMatch(/\.errorDiagnostics h3 span\s*\{[^}]*color:\s*var\(--danger-strong\);/s);
    expect(css).toMatch(/\.warningDiagnostics h3 span\s*\{[^}]*color:\s*var\(--warning\);/s);
  });
});
