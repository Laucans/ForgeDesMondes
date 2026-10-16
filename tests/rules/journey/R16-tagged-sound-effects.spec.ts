import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: trois sons tagués
// when: le DM filtre par tag et en joue deux à la fois
// then: chacun respecte son volume propre et le volume global, « Tout arrêter » coupe tout, et aucun son ne sort de la player view.
rule('R16', ['screen:soundbox', 'screen:dm-view', 'screen:player-view', 'micro-ui:SoundLibrary', 'micro-ui:LiveSoundbox'], "Soundbox d'effets tagués", async ({ page, context }) => {
  // The soundbox holds three tagged sounds only: the door, the crowd and the mug.
  await open(page, 'soundbox');
  const library = component(page, 'SoundLibrary');
  const keep = ['Porte qui grince', 'Bruit de foule en taverne', 'Chope qui se brise'];
  for (const sound of fx('sounds').sounds) {
    if (keep.includes(sound.name)) continue;
    await library.getByRole('row').filter({ hasText: sound.name }).getByRole('button', { name: 'Modifier' }).click();
    await library.getByRole('button', { name: 'Supprimer' }).click();
  }
  await expect(library.getByRole('row').filter({ hasText: '#' })).toHaveCount(keep.length);

  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Soundbox' }).click();
  const box = component(page, 'LiveSoundbox');
  const door = box.getByRole('button', { name: /Porte qui grince/ });
  const crowd = box.getByRole('button', { name: /Bruit de foule/ });
  const mug = box.getByRole('button', { name: /Chope qui se brise/ });

  // Filtered by the tag « taverne », the crowd and the mug remain: each gets its own volume, the master too.
  await box.getByRole('button', { name: '#taverne' }).click();
  await expect(door).toBeHidden();
  await expect(crowd).toBeVisible();
  await expect(mug).toBeVisible();
  await box.getByLabel('Volume de Bruit de foule en taverne').fill('40');
  await box.getByLabel('Volume de Chope qui se brise').fill('60');
  await box.getByLabel('Volume global').fill('50');

  // Both are played at once.
  await crowd.click();
  await mug.click();
  await expect(box.getByText(/2 sons en cours/)).toBeVisible();

  // Each plays at its own volume times the master's (the page's audio elements carry the output volume).
  const heard = () => page.evaluate(() => [...document.querySelectorAll('audio')].filter((a) => !a.paused).map((a) => a.volume).sort((x, y) => x - y));
  await expect.poll(async () => (await heard()).length).toBe(2);
  const [low, high] = await heard();
  expect(low).toBeCloseTo(0.4 * 0.5, 2);
  expect(high).toBeCloseTo(0.6 * 0.5, 2);

  // Nothing comes out of the player view.
  const player = await context.newPage();
  await open(player, 'player-view');
  expect(await player.evaluate(() => [...document.querySelectorAll('audio, video')].filter((m) => !(m as HTMLMediaElement).paused).length)).toBe(0);

  // « Tout arrêter » cuts everything.
  await page.bringToFront();
  await box.getByRole('button', { name: 'Tout arrêter' }).click();
  await expect(box.getByText(/son[s]? en cours/)).toHaveCount(0);
  expect(await heard()).toEqual([]);
});
