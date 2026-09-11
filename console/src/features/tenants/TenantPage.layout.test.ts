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
const css = fileSystem.readFileSync("src/features/tenants/TenantPage.module.css", "utf8");

describe("Tenant Component rows", () => {
  it("keeps trailing actions on the identity row instead of wrapping under status", () => {
    expect(css).toMatch(/grid-template-areas:\s*"icon content actions"/s);
    expect(css).not.toMatch(/"icon content"\s*"\. actions"/s);
    expect(css).toMatch(/\.componentIdentity strong\s*\{[^}]*text-overflow:\s*ellipsis/s);
  });
});

describe("Tenant home copy", () => {
  it("keeps Copy Tenant Home on the same 24px hit area as Visual help", () => {
    expect(css).toMatch(
      /\.componentHomeCopy\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*min-width:\s*24px;[^}]*min-height:\s*24px/s,
    );
  });

  it("keeps the home path and copy visible when the detail pane is under 900px", () => {
    expect(css).not.toMatch(
      /@container \(max-width: 900px\)[\s\S]*\.componentHome\s*\{[^}]*display:\s*none/s,
    );
    expect(css).toMatch(/\.componentHome\s*\{[^}]*display:\s*flex/s);
  });
});
