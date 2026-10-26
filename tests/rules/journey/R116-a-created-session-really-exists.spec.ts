import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, open } from './helpers';

// given: l'écran campagne ouvert sur la campagne « Les Héritiers de la Tour d’Ébène » par son adresse
// when: le DM y crée une séance titrée
// then: la séance apparaît dans la liste des séances de cette campagne et y est encore après rechargement
rule('R116', ['screen:campagne', 'micro-ui:SessionList', 'data-capability:session.createSession', 'capability:session.listSessions'], "Une séance créée existe vraiment", async ({ page }) => {
  // Opened on its campaign by the address; no other rule's test works in this campaign.
  await open(page, 'campagne', '?campaign=heritiers-ebene');
  const title = `Le pont des Pendus ${Date.now()}`;

  // The DM creates a titled session.
  await component(page, 'SessionList').getByRole('button', { name: 'Nouvelle séance' }).click();
  const dialog = page.getByRole('dialog', { name: 'Nouvelle séance' });
  await dialog.getByLabel('Titre').fill(title);
  await dialog.getByRole('button', { name: 'Créer la séance' }).click();

  // It is in the list of that campaign's sessions.
  await expect(component(page, 'SessionList')).toContainText(title);

  // And still there once the page is loaded again.
  await page.reload();
  await expect(component(page, 'SessionList')).toContainText(title);
});
