import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  base: "/_aibox/traffic/",
  plugins: [react()],
  build: {
    outDir: "../../assets",
    emptyOutDir: false,
    assetsDir: ".",
    rollupOptions: {
      input: {
        traffic: "./index.html",
      },
      output: {
        entryFileNames: "traffic.js",
        chunkFileNames: "traffic-[name].js",
        assetFileNames: (assetInfo) =>
          assetInfo.name?.endsWith(".css") ? "traffic.css" : "traffic-[name][extname]",
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    css: true,
    exclude: [...configDefaults.exclude, "e2e/**"],
  },
});
