import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: la fiche d'un PNJ dont la description a été modifiée sans être enregistrée
// when: le DM clique sur un autre élément du rail et choisit tour à tour « Rester », « Abandonner » puis, après une nouvelle modification, « Enregistrer et continuer »
// then: « Rester » garde la fiche et la modification affichées, « Abandonner » ouvre l'élément choisi et la modification est perdue, et « Enregistrer et continuer » enregistre la modification avant d'ouvrir l'élément choisi
rule('R95', ['screen:fiche-entite', 'micro-ui:EntityEditor', 'micro-ui:CampaignElements'], "Confirmation avant de quitter une fiche modifiée", async ({ page }) => {
  const entities = fx('entities');
  const original = entities.characters.find((c: any) => c.id === 'morlox').description;
  await open(page, 'fiche-entite', '?id=morlox');
  const editor = component(page, 'EntityEditor');
  const rail = page.getByRole('navigation', { name: 'Éléments de la campagne' });
  const description = editor.getByLabel('Description', { exact: true });
  const bar = (name: string) => editor.getByRole('button', { name });

  // « Rester »: the sheet and its change stay as they are.
  await description.fill('Première modification');
  await rail.getByRole('link', { name: /Gaspard/ }).click();
  await bar('Rester').click();
  await expect(editor.getByLabel('Nom')).toHaveValue('Morlox');
  await expect(description).toHaveValue('Première modification');

  // « Abandonner »: the chosen element opens, the change is lost.
  await rail.getByRole('link', { name: /Gaspard/ }).click();
  await bar('Abandonner').click();
  await expect(editor.getByLabel('Nom')).toHaveValue(/Gaspard/);
  await rail.getByRole('link', { name: /Morlox/ }).click();
  await expect(description).toHaveValue(original);

  // A new change, « Enregistrer et continuer »: saved, then the chosen element opens.
  await description.fill('Seconde modification');
  await rail.getByRole('link', { name: /Gaspard/ }).click();
  await bar('Enregistrer et continuer').click();
  await expect(editor.getByLabel('Nom')).toHaveValue(/Gaspard/);
  await rail.getByRole('link', { name: /Morlox/ }).click();
  await expect(description).toHaveValue('Seconde modification');
});
