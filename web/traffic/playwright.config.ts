import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  use: {
    baseURL: "http://127.0.0.1:4173/_aibox/traffic/",
    contextOptions: { reducedMotion: "reduce" },
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "desktop-chrome",
      testMatch: "**/*.chrome.spec.ts",
      use: { ...devices["Desktop Chrome"], channel: "chrome" },
    },
    {
      name: "desktop-firefox",
      testMatch: "**/*.smoke.spec.ts",
      use: { ...devices["Desktop Firefox"] },
    },
    {
      name: "desktop-webkit",
      testMatch: "**/*.smoke.spec.ts",
      use: { ...devices["Desktop Safari"] },
    },
  ],
  webServer: {
    command: "npm exec vite -- --host 127.0.0.1 --port 4173",
    url: "http://127.0.0.1:4173/_aibox/traffic/",
    stdout: "pipe",
  },
});
