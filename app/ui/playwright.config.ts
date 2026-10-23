import { defineConfig } from '@playwright/test';

// Screenshot baselines of every screen in Hebrew and Arabic, plus English for
// reference (brief 13 "RTL visual checks"), against the in-memory mock.
// @playwright/test is pinned to the release whose Chromium is preinstalled
// (revision 1194); CI installs exactly that browser.
export default defineConfig({
  testDir: 'e2e',
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  fullyParallel: true,
  workers: process.env.CI ? 2 : undefined,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never', outputFolder: 'e2e-report' }]] : 'list',
  outputDir: 'e2e-results',
  expect: {
    toHaveScreenshot: {
      // Font rasterisation differs a little between machines; layout bugs don't hide in 1.5%.
      maxDiffPixelRatio: 0.015,
      animations: 'disabled',
      caret: 'hide',
    },
  },
  use: {
    baseURL: 'http://localhost:4173/',
    viewport: { width: 1280, height: 800 },
    timezoneId: 'UTC',
    browserName: 'chromium',
  },
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    url: 'http://localhost:4173/',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
