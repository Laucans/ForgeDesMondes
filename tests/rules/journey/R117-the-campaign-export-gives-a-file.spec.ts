import { expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { rule } from './kit';
import { component, open } from './helpers';

// given: l'écran campagne ouvert sans campagne dans l'adresse
// when: le DM exporte l'archive de la campagne
// then: le navigateur enregistre un fichier JSON : l'archive « campaign-archive » de la campagne affichée, qui porte son nom
rule('R117', ['screen:campagne', 'micro-ui:CampaignOverview', 'data-capability:campaign.exportCampaign', 'capability:campaign.getCampaign'], "L'export de la campagne donne un fichier", async ({ page }) => {
  // No campaign in the address: the screen shows the default one.
  await open(page, 'campagne');
  const overview = component(page, 'CampaignOverview');
  const heading = overview.getByRole('heading', { level: 1 });
  await expect(heading).not.toBeEmpty();
  const shown = ((await heading.textContent()) ?? '').trim();

  // The DM exports the archive: the browser saves a file.
  const saved = page.waitForEvent('download');
  await overview.getByRole('button', { name: /Exporter l.archive/ }).click();
  const download = await saved;

  // It is a JSON file: the campaign-archive of the campaign shown, which bears its name.
  expect(download.suggestedFilename()).toMatch(/\.json$/i);
  const path = await download.path();
  const archive = JSON.parse(readFileSync(path, 'utf8'));
  expect(archive.format).toBe('campaign-archive');
  expect(archive.campaign.name).toBe(shown);
});
