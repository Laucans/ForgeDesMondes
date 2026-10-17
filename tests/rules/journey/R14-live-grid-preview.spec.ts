import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une battlemap téléversée
// when: le DM règle taille, décalage, orientation, couleur et opacité de la grille
// then: l'aperçu se met à jour en direct et un hex vaut toujours 5 pieds.
rule('R14', ['screen:medias', 'micro-ui:MediaLibrary', 'micro-ui:GridSettings'], "Médias téléversés et grille hexagonale réglée", async ({ page }) => {
  await open(page, 'medias');
  await component(page, 'MediaLibrary').getByRole('button', { name: 'Régler la grille' }).first().click();
  const grid = component(page, 'GridSettings');
  const path = grid.locator('.hm-grid-path').first();
  const look = () => path.evaluate((el) => `${el.getAttribute('d')}|${getComputedStyle(el).stroke}|${getComputedStyle(el).opacity}|${getComputedStyle(el.parentElement as Element).opacity}`);

  // One hex stays 5 feet whatever the grid.
  await expect(grid.getByText(/1 hex = 5 pi/)).toBeVisible();
  const settings: Array<() => Promise<unknown>> = [
    () => grid.getByLabel('Taille d’un hex').fill('48'),
    () => grid.getByLabel('Décalage horizontal').fill('30'),
    () => grid.getByLabel('Décalage vertical').fill('25'),
    () => grid.getByLabel('Côté plat en haut').check(),
    () => grid.getByLabel('Couleur de la grille').selectOption({ label: 'Vert' }),
    () => grid.getByLabel('Opacité').fill('20'),
  ];
  for (const change of settings) {
    const before = await look();
    await change();
    await expect.poll(look, { timeout: 1000 }).not.toBe(before);
    await expect(grid.getByText(/1 hex = 5 pi/)).toBeVisible();
  }
});
