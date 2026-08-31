import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  use: {
    baseURL: "http://127.0.0.1:4173/_aibox/ui/requests/",
    contextOptions: { reducedMotion: "reduce" },
    trace: "retain-on-failure",
  },
  projects: [
    {
      // Bundled Chromium rather than an installed Chrome channel, because
      // Google ships no Linux arm64 Chrome and this project must run the same
      // way on a host and inside the Linux image.
      name: "chromium",
      testMatch: "**/*.chromium.spec.ts",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "npm exec vite -- --host 127.0.0.1 --port 4173",
    url: "http://127.0.0.1:4173/_aibox/ui/requests/",
    stdout: "pipe",
  },
});
