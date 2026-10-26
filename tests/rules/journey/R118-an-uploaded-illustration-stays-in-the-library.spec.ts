import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, open } from './helpers';

// A 1 × 1 PNG: the card measures the image it is given.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');

// given: l'écran médias ouvert sans campagne dans l'adresse
// when: le DM y téléverse une illustration sous un titre qu'aucun autre média ne porte
// then: l'illustration apparaît sous ce titre parmi les illustrations de la médiathèque et y est encore après rechargement
rule('R118', ['screen:medias', 'micro-ui:MediaLibrary', 'data-capability:media.uploadMedia', 'capability:media.listMedia'], "Une illustration téléversée reste dans la médiathèque", async ({ page }) => {
  // No campaign in the address: the screen shows the default one.
  await open(page, 'medias');
  const library = component(page, 'MediaLibrary');
  // A title no other medium bears.
  const title = `Vitrail brisé ${Date.now()}`;

  // The DM uploads an illustration (never a battlemap) under that title.
  await library.locator('input[type="file"]').setInputFiles({ name: 'vitrail-brise.png', mimeType: 'image/png', buffer: PNG });
  await library.getByRole('radio', { name: /Illustration/ }).check();
  await library.getByLabel('Titre').fill(title);
  await library.getByRole('button', { name: 'Ajouter à la campagne' }).click();

  // It is among the library's illustrations under that title.
  const illustrations = library.getByRole('region', { name: 'Illustrations' });
  await expect(illustrations).toContainText(title);

  // And still there once the page is loaded again.
  await page.reload();
  await expect(component(page, 'MediaLibrary').getByRole('region', { name: 'Illustrations' })).toContainText(title);
});
