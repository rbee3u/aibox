import { defineConfig } from "vitest/config";

const domTypeScriptTests = [
  "src/api/connect.test.ts",
  "src/api/requests.test.ts",
  "src/api/transport.test.ts",
  "src/app/routing/useConsoleRouter.test.ts",
  "src/features/requests/detail/bodyPresentation.test.ts",
  "src/features/requests/requestFormat.test.ts",
];

export default defineConfig({
  base: "/_aibox/ui/",
  publicDir: false,
  css: {
    // Handles cross-file CSS Module composition without PostCSS's missing-source warning.
    transformer: "lightningcss",
  },
  resolve: {
    alias: {
      "@": new URL("./src", import.meta.url).pathname,
    },
  },
  build: {
    assetsInlineLimit: () => true,
    cssCodeSplit: false,
    rolldownOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: "assets/index.js",
        assetFileNames: "assets/style.css",
      },
    },
  },
  test: {
    pool: "threads",
    projects: [
      {
        extends: true,
        test: {
          name: "node",
          environment: "node",
          isolate: false,
          setupFiles: "./src/test/reset.ts",
          include: ["src/**/*.test.ts"],
          exclude: domTypeScriptTests,
        },
      },
      {
        extends: true,
        test: {
          name: "dom",
          environment: "jsdom",
          setupFiles: "./src/test/setup.ts",
          include: ["src/**/*.test.tsx", ...domTypeScriptTests],
        },
      },
    ],
  },
});
