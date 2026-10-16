// What the journey rules' tests share: opening a screen with a query, and the mock-up's fixtures
// as the data the human validated. A test still reaches the product by identifier only.
import type { Locator, Page } from '@playwright/test';
import { fixture } from './kit';

/** A fixture of the mock-up, untyped: the test reads what it needs. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const fx = (name: string): any => fixture(name);

/** Opens a screen by its name, with a query string, where the runtime serves it. */
export async function open(page: Page, screen: string, query = ''): Promise<void> {
  const route = (process.env.RULES_SCREEN_ROUTE || '/screens/<screen>').replace('<screen>', encodeURIComponent(screen));
  await page.goto(route + query);
}

/** A Micro-UI on the page, by its name. */
export const component = (page: Page, name: string): Locator => page.locator(`[data-component="${name}"]`).first();
