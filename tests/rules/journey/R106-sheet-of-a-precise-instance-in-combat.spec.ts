import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un combat en cours avec Bandit 1, Bandit 2 et Bandit 3
// when: le DM clique sur Bandit 2 dans le carrousel et retire 7 PV
// then: la fiche ouverte est celle de Bandit 2, seuls les PV de Bandit 2 baissent de 7, et ceux de Bandit 1 et Bandit 3 restent inchangés
rule('R106', ['screen:dm-view', 'micro-ui:Battlemap', 'micro-ui:TableSheet'], "Fiche d'une instance précise en combat", async ({ page }) => {
  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Battlemap' }).click();
  const map = component(page, 'Battlemap');

  // The combat is under way with the three bandits.
  await map.getByRole('button', { name: 'Lancer le combat' }).click();
  await page.getByRole('button', { name: 'Commencer le combat' }).click();
  const carousel = map.getByLabel('Ordre d’initiative');
  await expect(carousel).toBeVisible();

  // A click on Bandit 2 in the carousel opens its own sheet; 7 PV are taken off.
  await carousel.locator('.kit-token__item').filter({ hasText: 'Bandit 2' }).click();
  const sheet = map.getByRole('complementary', { name: 'Bandit 2' });
  await expect(sheet).toBeVisible();
  const hp = (scope: typeof sheet) => scope.getByText(/\d+ \/ \d+ PV/);
  await expect(hp(sheet)).toContainText('11 / 11');
  await sheet.getByLabel('Points de vie').fill('7');
  await sheet.getByRole('button', { name: /Dégâts/ }).click();
  await expect(hp(sheet)).toContainText('4 / 11');

  // The two other instances are untouched.
  for (const other of ['Bandit 1', 'Bandit 3']) {
    await carousel.locator('.kit-token__item').filter({ hasText: other }).click();
    const own = map.getByRole('complementary', { name: other });
    await expect(hp(own)).toContainText('11 / 11');
  }
});
