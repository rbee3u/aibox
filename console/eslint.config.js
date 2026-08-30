import eslint from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const features = ["overview", "tenants", "configs", "sessions", "requests"];

/**
 * Console dependencies point inward from app to features and from features to
 * domain/api/shared. API and shared may depend on domain, while domain depends
 * on none of the other layers. `src/test` is exempt because harnesses compose pages.
 *
 * `features/common` sits between the feature pages and those inner layers: it
 * holds the pieces several features share that need both an `api/` wire type
 * and a `shared/ui` type, which `shared/` itself may not import. It is
 * deliberately absent from `features` above, so it is not subject to the
 * features-may-not-import-each-other rule and every feature may import it.
 * Its own boundary below keeps it from importing any feature back.
 */
function layerBoundary(directory, forbidden) {
  return {
    files: [`src/${directory}/**/*.{ts,tsx}`],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: forbidden,
        },
      ],
    },
  };
}

const requireAliases = {
  regex: "^\\.{1,2}/",
  message: "Use the @/ aliases inside src so layer boundaries remain visible to ESLint.",
};

const featureBoundaries = features.map((feature) =>
  layerBoundary(`features/${feature}`, [
    {
      group: features.filter((other) => other !== feature).map((other) => `@/features/${other}/*`),
      message:
        "Features may not import each other. Move the shared part into features/common/, shared/, or api/ instead.",
    },
    {
      group: ["@/app/*"],
      message:
        "Features may not import the app shell. The shell passes what a page needs as props.",
    },
    {
      group: ["@/api/transport", "@/api/generated/*"],
      message:
        "Features must use a domain API adapter. Keep generated wire values and HTTP details inside api/.",
    },
    requireAliases,
  ]),
);

export default defineConfig(
  eslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    extends: tseslint.configs.recommendedTypeChecked,
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [reactHooks.configs.flat.recommended],
  },
  layerBoundary("api", [
    {
      group: ["@/app/*", "@/features/*", "@/shared/*"],
      message:
        "The Control API client is the innermost layer and may only import from @/api. Move shared helpers it needs into @/api.",
    },
    requireAliases,
  ]),
  layerBoundary("shared", [
    {
      group: ["@/app/*", "@/features/*", "@/api/*"],
      message:
        "Shared code may depend only on shared/ or domain/. Pass adapter values and loaders in.",
    },
    requireAliases,
  ]),
  layerBoundary("domain", [
    {
      group: ["@/app/*", "@/features/*", "@/shared/*", "@/api/*"],
      message: "Domain invariants are framework-free and may not depend on other layers.",
    },
    requireAliases,
  ]),
  layerBoundary("features/common", [
    {
      group: [...features.map((feature) => `@/features/${feature}/*`), "@/app/*"],
      message:
        "features/common is shared by every feature and may not import one back, or the shared layer becomes a dependency cycle. It may import api/, shared/, and domain/.",
    },
    {
      group: ["@/api/transport", "@/api/generated/*"],
      message:
        "features/common must use a domain API adapter. Keep generated wire values and HTTP details inside api/.",
    },
    requireAliases,
  ]),
  ...featureBoundaries,
);
