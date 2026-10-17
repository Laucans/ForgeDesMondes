import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un brief raconté au micro
// when: il est traité
// then: une liste ordonnée de scènes apparaît, chacune liée à ses lieux, PNJ, événements et médias, réordonnable et modifiable à la main.
rule('R11', ['screen:preparation-seance', 'micro-ui:OralBrief', 'micro-ui:SceneBoard'], "Brief oral découpé en scènes", async ({ page }) => {
  const brief = fx('OralBrief');
  const board = fx('SceneBoard');
  const entities = fx('entities');
  const media = fx('media');
  const nameOf = (id: string): string =>
    [...entities.characters, ...entities.places, ...entities.events].find((e: any) => e.id === id)?.name
    ?? [...media.battlemaps, ...media.illustrations].find((m: any) => m.id === id)?.title;
  await open(page, 'preparation-seance');

  // The brief is told, then processed.
  const oral = component(page, 'OralBrief');
  await oral.getByRole('button', { name: /Brief oral/ }).click();
  await oral.getByRole('button', { name: 'Terminer et analyser' }).click();
  await expect(oral.getByText(/scènes créées/)).toBeVisible({ timeout: 20_000 });

  // An ordered list of scenes, each linked to its places, NPCs, events and media.
  const scenes = component(page, 'SceneBoard');
  const titles = board.scenes.map((s: any) => s.title);
  await expect(scenes.getByRole('heading', { level: 3 })).toHaveText(titles);
  for (const scene of board.scenes) {
    const card = scenes.getByRole('listitem').filter({ has: page.getByRole('heading', { name: scene.title }) });
    for (const id of [...scene.places, ...scene.characters, ...scene.events, ...scene.media]) {
      await expect(card).toContainText(nameOf(id));
    }
  }

  // Reordered by hand: the second scene goes up.
  await scenes.getByRole('button', { name: 'Monter' }).nth(1).click();
  await expect(scenes.getByRole('heading', { level: 3 })).toHaveText([titles[1], titles[0], titles[2]]);

  // Completed by hand: a scene added, notes written.
  await scenes.getByRole('button', { name: 'Ajouter une scène' }).click();
  await expect(scenes.getByRole('heading', { level: 3 })).toHaveCount(titles.length + 1);
  const notes = scenes.getByLabel('Notes').first();
  await notes.fill('Une note écrite à la main.');
  await expect(notes).toHaveValue('Une note écrite à la main.');
});
