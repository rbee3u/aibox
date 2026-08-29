import { defineConfig } from "vitest/config";

type NodeProcess = { env: Record<string, string | undefined> };
const nodeProcess = (globalThis as typeof globalThis & { process: NodeProcess }).process;
const outputDirectory = nodeProcess.env.AIBOX_CONSOLE_OUT_DIR ?? "../assets";

export default defineConfig({
  base: "/_aibox/ui/",
  publicDir: false,
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
    rolldownOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: "console.js",
        assetFileNames: "console.css",
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
