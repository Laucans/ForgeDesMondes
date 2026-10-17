import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une séance en cours
// when: le DM ouvre la DM view
// then: les éléments de la séance sont en tête à gauche, les scènes défilent une à une, et un clic ouvre au centre la fiche en lecture ou la battlemap de la scène.
rule('R17', ['screen:dm-view', 'micro-ui:ElementsPanel', 'micro-ui:TableSheet', 'micro-ui:Battlemap', 'micro-ui:LiveSessionBar'], "Conduite de la séance depuis la DM view", async ({ page }) => {
  const board = fx('SceneBoard');
  const entities = fx('entities');
  await open(page, 'dm-view');
  const panel = component(page, 'ElementsPanel');

  // On the left: the session's elements first, then the whole campaign, scene after scene.
  const session = panel.getByRole('region', { name: 'Séance en cours' });
  const campaign = panel.getByRole('region', { name: 'Toute la campagne' });
  const sessionBox = await session.boundingBox();
  const campaignBox = await campaign.boundingBox();
  expect((sessionBox?.y ?? Infinity) < (campaignBox?.y ?? 0), 'the session comes first').toBe(true);
  const scenes = panel.locator('details[data-scene-index]');
  await expect(scenes).toHaveCount(board.scenes.length);
  await expect(scenes.first()).toHaveAttribute('open', '');
  const bar = component(page, 'LiveSessionBar');
  await bar.getByRole('button', { name: new RegExp(`Scène 2`) }).click();
  await expect(scenes.nth(1)).toHaveAttribute('open', '');

  // A click opens the sheet, optimised for reading, in the centre.
  const gaspard = entities.characters.find((c: any) => c.id === 'gaspard');
  await panel.getByText(gaspard.name).first().click();
  const sheet = component(page, 'TableSheet');
  await expect(sheet.getByRole('heading', { name: gaspard.name })).toBeVisible();
  await expect(sheet.getByRole('link', { name: 'Modifier' })).toBeVisible();

  // The scene's battlemap opens in the centre as well.
  await page.getByRole('tab', { name: 'Battlemap' }).click();
  await expect(component(page, 'Battlemap')).toBeVisible();
});
