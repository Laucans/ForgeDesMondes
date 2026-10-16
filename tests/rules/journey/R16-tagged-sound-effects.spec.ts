import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: trois sons tagués
// when: le DM filtre par tag et en joue deux à la fois
// then: chacun respecte son volume propre et le volume global, « Tout arrêter » coupe tout, et aucun son ne sort de la player view.
rule('R16', ['screen:soundbox', 'screen:dm-view', 'micro-ui:SoundLibrary', 'micro-ui:LiveSoundbox'], "Soundbox d'effets tagués", async ({ page, context }) => {
  // A sound added to the library, played from the DM view.
  await open(page, 'soundbox');
  const library = component(page, 'SoundLibrary');
  await library.getByRole('button', { name: /Utiliser l’exemple/ }).click();
  await library.getByLabel('Nom', { exact: true }).first().fill('Volet de test');
  await library.getByLabel('Tags').first().fill('test');
  await library.getByRole('button', { name: 'Ajouter à la soundbox' }).click();

  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Soundbox' }).click();
  const box = component(page, 'LiveSoundbox');
  await box.getByRole('button', { name: '#test' }).click();
  await box.getByRole('button', { name: /Volet de test/ }).click();
  await expect(box.getByText(/en cours/)).toBeVisible();

  // Nothing comes out of the player view.
  const player = await context.newPage();
  await open(player, 'player-view');
  await expect(box.getByText(/uniquement de cette vue/)).toBeVisible();
  expect(await player.evaluate(() => [...document.querySelectorAll('audio, video')].filter((m) => !(m as HTMLMediaElement).paused).length)).toBe(0);
  await expect(player.getByText(/en cours/)).toHaveCount(0);
});
