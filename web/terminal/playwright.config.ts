import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  // CI's criteria job reads which tests passed from the JSON reports (D95). verify:pr runs Playwright
  // once per suite, so each run writes its own file.
  reporter: process.env.CI
    ? [
        ['html', { open: 'never' }],
        ['line'],
        ['json', { outputFile: `test-reports/playwright-${process.pid}.json` }],
      ]
    : 'list',
  outputDir: 'test-results',
  snapshotPathTemplate: '{snapshotDir}/{testFilePath}-snapshots/{arg}{ext}',
  expect: {
    toHaveScreenshot: {
      animations: 'disabled',
      maxDiffPixels: 100,
    },
  },
  use: {
    baseURL: 'http://127.0.0.1:4173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    // Cloud sessions that can't download Playwright's pinned Chromium point this at a preinstalled one
    // (.claude/hooks/session-start.sh). Visual baselines still come from CI's Chromium.
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM_PATH
      ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH }
      : {},
  },
  webServer: [
    {
      // The default build: the fixture source.
      command: 'npm run preview -- --host 127.0.0.1',
      url: 'http://127.0.0.1:4173',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
    {
      // The same app built for the live source, by env alone (#63). Tests play the API server
      // with page.routeWebSocket; nothing listens on LIVE_API_URL.
      command: `node scripts/build-live.mjs && npx vite preview --outDir dist-live --host 127.0.0.1 --port 4174 --strictPort`,
      url: 'http://127.0.0.1:4174',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
  ],
  projects: [
    {
      name: 'chromium',
      testIgnore: /live\//,
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'chromium-live',
      testMatch: /live\/.*\.spec\.ts/,
      use: { ...devices['Desktop Chrome'], baseURL: 'http://127.0.0.1:4174' },
    },
  ],
})
