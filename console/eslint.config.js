import eslint from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const features = ["overview", "tenants", "configs", "sessions", "requests"];

/**
 * Console dependencies point inward from app to features and from features to
 * domain/api/shared. API and shared may depend on domain, while domain depends
 * on none of the other layers. `src/test` is exempt because harnesses compose pages.
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

const featureBoundaries = features.map((feature) =>
  layerBoundary(`features/${feature}`, [
    {
      group: features.filter((other) => other !== feature).map((other) => `@/features/${other}/*`),
      message:
        "Features may not import each other. Move the shared part into shared/ or api/ instead.",
    },
    {
      group: ["@/app/*"],
      message:
        "Features may not import the app shell. The shell passes what a page needs as props.",
    },
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
  ]),
  layerBoundary("shared", [
    {
      group: ["@/app/*", "@/features/*", "@/api/*"],
      message:
        "Shared code may depend only on shared/ or domain/. Pass adapter values and loaders in.",
    },
  ]),
  layerBoundary("domain", [
    {
      group: ["@/app/*", "@/features/*", "@/shared/*", "@/api/*"],
      message: "Domain invariants are framework-free and may not depend on other layers.",
    },
  ]),
  ...featureBoundaries,
);
