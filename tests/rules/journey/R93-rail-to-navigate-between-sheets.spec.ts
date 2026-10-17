import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: la fiche d'un lieu ouverte, et un PJ « Elowen » portant l'alias « la rôdeuse » dans la campagne
// when: le DM tape « elowen », puis efface et tape « rodeuse » dans la recherche du rail, et clique sur Elowen
// then: Elowen figure dans les résultats pour chacune des deux saisies, sa fiche remplace celle du lieu sans rechargement de page, et l'URL porte son identifiant
rule('R93', ['screen:fiche-entite', 'micro-ui:CampaignElements', 'micro-ui:EntityEditor'], "Rail de navigation entre les fiches d'entité", async ({ page }) => {
  await open(page, 'fiche-entite', '?id=taverne-sanglier');
  const editor = component(page, 'EntityEditor');
  await expect(editor.getByLabel('Nom')).toHaveValue('La Taverne du Sanglier');
  await page.evaluate(() => { (window as any).__sameDocument = true; });
  const rail = page.getByRole('navigation', { name: 'Éléments de la campagne' });
  const search = rail.getByLabel('Rechercher');
  const elowen = rail.getByRole('link', { name: /Elowen/ });

  // « elowen », then « rodeuse »: Elowen is found by her name and by her alias, however approximate.
  await search.fill('elowen');
  await expect(elowen).toBeVisible();
  await search.fill('');
  await search.fill('rodeuse');
  await expect(elowen).toBeVisible();

  // A click loads her sheet in place, and the address carries her identifier.
  await elowen.click();
  await expect(editor.getByLabel('Nom')).toHaveValue('Elowen Ventargent');
  await expect(page).toHaveURL(/[?&]id=elowen\b/);
  expect(await page.evaluate(() => (window as any).__sameDocument === true), 'the page was reloaded').toBe(true);
});
