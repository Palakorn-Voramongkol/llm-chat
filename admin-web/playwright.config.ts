import { defineConfig, devices } from "@playwright/test";

const BASE_URL = process.env.ADMIN_WEB_URL ?? "http://localhost:3000";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  use: { baseURL: BASE_URL, trace: "on-first-retry" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  // Start admin-web for the redirect smoke; the full login path needs the
  // whole compose stack (admin-api + Zitadel) and runs under ADMIN_IT=1.
  webServer: process.env.ADMIN_IT
    ? undefined
    : {
        command: "pnpm dev",
        url: BASE_URL,
        reuseExistingServer: true,
        timeout: 60_000,
      },
});
