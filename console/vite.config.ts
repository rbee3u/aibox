import { defineConfig } from "vitest/config";

export default defineConfig({
  base: "/_aibox/ui/",
  publicDir: false,
  resolve: {
    alias: {
      "@": new URL("./src", import.meta.url).pathname,
    },
  },
  build: {
    outDir: "../assets",
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
