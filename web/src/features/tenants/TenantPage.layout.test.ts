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
const css = [
  "src/features/tenants/catalog/TenantCatalogPane.module.css",
  "src/features/tenants/components/TenantDetailPane.module.css",
  "src/features/tenants/mutation/TenantDialogs.module.css",
  "src/features/tenants/TenantPage.module.css",
  "src/features/tenants/components/components.module.css",
  "src/features/tenants/components/ComponentRowItem.module.css",
  "src/features/tenants/components/ComponentCatalogSkeleton.module.css",
]
  .map((path) => fileSystem.readFileSync(path, "utf8"))
  .join("\n");

describe("Tenant Component rows", () => {
  it("keeps trailing actions on the identity row instead of wrapping under status", () => {
    expect(css).toMatch(/grid-template-areas:\s*"icon content actions"/s);
    expect(css).not.toMatch(/"icon content"\s*"\. actions"/s);
    expect(css).toMatch(/\.componentIdentity strong\s*\{[^}]*text-overflow:\s*ellipsis/s);
  });
});

// Home visibility and copy-control geometry are checked on the rendered page
// in e2e/catalogs.chromium.spec.ts; the old selectors were unused by the view.

describe("Tenant header alignment", () => {
  it("aligns catalog toolbar and detail header heights to 56px", () => {
    expect(css).toMatch(
      /\.tenantCatalog\s*>\s*div:first-child\s*\{[^}]*height:\s*56px;[^}]*min-height:\s*56px/s,
    );
    expect(css).toMatch(/\.tenantDetailHeader\s*\{[^}]*height:\s*56px;[^}]*min-height:\s*56px/s);
  });
});
