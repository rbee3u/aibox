import { defineConfig } from "vitest/config";

type NodeProcess = { env: Record<string, string | undefined> };
const nodeProcess = (globalThis as typeof globalThis & { process: NodeProcess }).process;
const outputDirectory = nodeProcess.env.AIBOX_CONSOLE_OUT_DIR ?? "../assets";
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
    outDir: outputDirectory,
    // The shared assets directory also contains non-Vite inputs such as the Dockerfile.
    emptyOutDir: false,
    assetsInlineLimit: () => true,
    cssCodeSplit: false,
    // One embedded Console bundle; the gzip budget is the real size gate.
    chunkSizeWarningLimit: 8192,
    rolldownOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: "console.js",
        assetFileNames: "console.css",
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
