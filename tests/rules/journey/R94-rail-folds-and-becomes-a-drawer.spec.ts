import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: la fiche d'entité ouverte avec son rail déplié
// when: le DM replie le rail et recharge la page, puis réduit la fenêtre sous 960 px, ouvre le tiroir et le referme
// then: le rail est toujours replié après le rechargement, et sous 960 px le tiroir s'ouvre au-dessus de la fiche puis se referme sans la masquer
rule('R94', ['screen:fiche-entite', 'micro-ui:CampaignElements', 'micro-ui:EntityEditor'], "Rail repliable et tiroir sur petit écran", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await open(page, 'fiche-entite', '?id=morlox');
  const rail = page.getByRole('navigation', { name: 'Éléments de la campagne' });
  const search = rail.getByLabel('Rechercher');
  await expect(search).toBeVisible();

  // The button at the top left folds the rail to a thin strip; the page remembers it.
  await rail.getByTitle('Replier la liste').click();
  await expect(search).toBeHidden();
  const strip = await rail.boundingBox();
  expect(strip?.width ?? Infinity).toBeLessThan(120);
  await page.reload();
  await expect(search).toBeHidden();

  // Under 960 px the rail is a drawer, opened over the sheet and closed without hiding it.
  await page.setViewportSize({ width: 800, height: 900 });
  const editor = component(page, 'EntityEditor');
  await expect(rail).toBeHidden();
  const sheetBefore = await editor.boundingBox();
  await page.getByRole('button', { name: /^☰/ }).click();
  await expect(rail).toBeVisible();

  // Open, the drawer lies over the sheet and the sheet has not moved.
  const drawer = await rail.boundingBox();
  const sheetOpen = await editor.boundingBox();
  const overlaps = !!drawer && !!sheetOpen
    && drawer.x < sheetOpen.x + sheetOpen.width && drawer.x + drawer.width > sheetOpen.x
    && drawer.y < sheetOpen.y + sheetOpen.height && drawer.y + drawer.height > sheetOpen.y;
  expect(overlaps, 'the drawer opens over the sheet').toBe(true);
  expect(sheetOpen, 'the sheet did not move when the drawer opened').toEqual(sheetBefore);
  await rail.getByTitle('Fermer la liste').click();
  await expect(rail).toBeHidden();
  const covered = await editor.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + Math.min(r.height / 2, 200));
    return !top || !el.contains(top);
  });
  expect(covered, 'the sheet is hidden once the drawer is closed').toBe(false);
});
