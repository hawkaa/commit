import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./test",
  timeout: 60000,
  retries: 0,
  reporter: "list",
  projects: [
    {
      name: "chromium",
      // Extensions only work in headed Chromium (not Firefox/WebKit)
    },
  ],
});
