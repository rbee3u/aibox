import eslint from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const features = ["overview", "tenants", "configs", "sessions", "requests"];

/**
 * The Console is layered `app` -> `features` -> `shared` -> `api`. Each rule
 * below forbids the imports that would reverse an edge or let two features
 * depend on each other; `src/test` is exempt because harnesses compose pages.
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
      group: ["@/app/*", "@/features/*"],
      message:
        "Shared code may not depend on the app shell or a feature. Invert the dependency by passing values in.",
    },
  ]),
  ...featureBoundaries,
);
