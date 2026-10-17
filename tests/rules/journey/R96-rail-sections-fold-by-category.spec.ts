import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: la fiche d'entité ouverte avec son rail déplié et les sections PJ, PNJ, lieux et événements toutes ouvertes
// when: le DM clique sur le titre « PJ », puis clique de nouveau sur ce même titre
// then: au premier clic, les éléments PJ sont masqués, le triangle à gauche du titre indique que la section est fermée, et les sections PNJ, lieux et événements restent ouvertes ; au second clic, les éléments PJ réapparaissent et le triangle indique que la section est ouverte
rule('R96', ['screen:fiche-entite', 'micro-ui:CampaignElements'], "Sections du rail repliables par catégorie", async ({ page }) => {
  await open(page, 'fiche-entite', '?id=morlox');
  const rail = page.getByRole('navigation', { name: 'Éléments de la campagne' });
  const section = (name: string) => rail.getByRole('region', { name });
  const title = (name: RegExp) => rail.getByRole('button', { name });
  for (const name of ['PJ', 'PNJ', 'Lieux', 'Événements']) {
    await expect(section(name).getByRole('link').first()).toBeVisible();
  }

  // First click on « PJ »: its elements are hidden, its triangle points to « closed », the others stay open.
  await title(/^PJ/).click();
  await expect(section('PJ').getByRole('link')).toHaveCount(0);
  await expect(title(/^PJ/)).toContainText('▸');
  for (const name of ['PNJ', 'Lieux', 'Événements']) {
    await expect(section(name).getByRole('link').first()).toBeVisible();
  }

  // Second click: they come back and the triangle says « open ».
  await title(/^PJ/).click();
  await expect(section('PJ').getByRole('link').first()).toBeVisible();
  await expect(title(/^PJ/)).toContainText('▾');
});
