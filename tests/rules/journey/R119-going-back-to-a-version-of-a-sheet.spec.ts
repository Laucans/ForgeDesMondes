import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: la fiche de Mère Ombeline, dont le DM vient de changer la description et de l'enregistrer
// when: il revient à la version précédente depuis l'historique
// then: la fiche reprend sa description d'origine, une nouvelle version apparaît en tête de l'historique, et l'une comme l'autre sont encore là après rechargement
rule('R119', ['screen:fiche-entite', 'micro-ui:EntityEditor', 'micro-ui:VersionHistory', 'data-capability:campaign.saveEntity', 'data-capability:campaign.restoreEntityVersion', 'capability:campaign.getEntity', 'capability:campaign.listEntityVersions'], "Revenir à une version d'une fiche", async ({ page }) => {
  const original = fx('entities').characters.find((c: any) => c.id === 'ombeline').description;
  // Mère Ombeline: no other rule's test opens her sheet.
  await open(page, 'fiche-entite', '?id=ombeline');
  const editor = component(page, 'EntityEditor');
  const description = editor.getByLabel('Description', { exact: true });
  await expect(description).toHaveValue(original);

  // The DM changes the description and saves it.
  await description.fill(`Herboriste devenue sourde au bruit des cloches ${Date.now()}`);
  await editor.getByRole('button', { name: 'Enregistrer la fiche' }).click();

  // The history, opened from its trigger (« Mis à jour » once the changes are unseen).
  const history = component(page, 'VersionHistory');
  const trigger = history.getByRole('button', { name: /Historique des versions|Mis à jour/ });
  const dialog = page.getByRole('dialog', { name: /Versions de/ });
  const versions = dialog.getByRole('button', { name: 'Voir le diff' });
  await trigger.click();
  await expect(versions.first()).toBeVisible();
  const before = await versions.count();

  // Back to the version before: the first button offered belongs to the version under the newest.
  await dialog.getByRole('button', { name: 'Revenir à cette version' }).first().click();

  // The sheet takes the original description back, and a new version heads the history.
  await expect(versions).toHaveCount(before + 1);
  const head = dialog.getByRole('listitem').filter({ has: page.getByRole('button', { name: 'Voir le diff' }) }).first();
  await expect(head).toContainText(/Retour à/);
  await expect(description).toHaveValue(original);

  // Both are still there once the page is loaded again.
  await page.reload();
  await expect(component(page, 'EntityEditor').getByLabel('Description', { exact: true })).toHaveValue(original);
  await component(page, 'VersionHistory').getByRole('button', { name: /Historique des versions|Mis à jour/ }).click();
  await expect(versions).toHaveCount(before + 1);
  await expect(head).toContainText(/Retour à/);
});
