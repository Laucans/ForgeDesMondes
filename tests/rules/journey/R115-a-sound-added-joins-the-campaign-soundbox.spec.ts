import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, open } from './helpers';

// given: la soundbox ouverte sur la campagne « Les Héritiers de la Tour d’Ébène » par son adresse
// when: le DM y ajoute un son nommé et tagué
// then: le son apparaît dans la bibliothèque de cette campagne et y est encore après rechargement, sans refus « un son nomme sa campagne » ni attente de revue
rule('R115', ['screen:soundbox', 'micro-ui:SoundLibrary', 'data-capability:soundbox.addSound', 'capability:soundbox.listSounds'], "Un son ajouté rejoint la soundbox de la campagne", async ({ page }) => {
  // Opened on its campaign by the address; no other rule's test works in this campaign.
  await open(page, 'soundbox', '?campaign=heritiers-ebene');
  const library = component(page, 'SoundLibrary');
  const form = library.getByRole('form', { name: 'Téléverser un son' });
  const name = `Tonnerre lointain ${Date.now()}`;

  // The DM adds a named and tagged sound.
  await form.locator('input[type="file"]').setInputFiles({ name: 'tonnerre-lointain.ogg', mimeType: 'audio/ogg', buffer: Buffer.from('OggS-rules-sound') });
  await form.getByLabel('Nom').fill(name);
  await form.getByLabel('Tags').fill('orage, ciel');
  await form.getByRole('button', { name: 'Ajouter à la soundbox' }).click();

  // It is in the library of that campaign at once, neither refused nor kept waiting.
  const row = library.getByRole('row').filter({ hasText: name });
  await expect(row).toBeVisible();
  await expect(row).toContainText('orage');
  await expect(row).toContainText('ciel');
  await expect(library).not.toContainText('un son nomme sa campagne');

  // And still there once the page is loaded again.
  await page.reload();
  await expect(component(page, 'SoundLibrary').getByRole('row').filter({ hasText: name })).toBeVisible();
});
