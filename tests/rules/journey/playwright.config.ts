// The rules' journey tests run in a browser against the product as the
// rules' runtime serves it (`scripts/rules-runtime.sh serve` exports
// RULES_APP_URL); every outcome goes to RULES_STATUS through the status
// reporter. Installed by `harness init-repo`.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  // One worker: every journey walks the same product and the same scratch
  // database, and the product keeps one state (e.g. one last connection test),
  // so two journeys in lock-step read each other's writes.
  workers: 1,
  use: { baseURL: process.env.RULES_APP_URL || undefined },
  reporter: [['list'], ['./status-reporter.ts']],
});
