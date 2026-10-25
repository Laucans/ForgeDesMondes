import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un brief raconté au micro
// when: il est traité
// then: une liste ordonnée de scènes apparaît, chacune liée à ses lieux, PNJ, événements et médias, réordonnable et modifiable à la main.
rule('R11', ['screen:preparation-seance', 'micro-ui:OralBrief', 'micro-ui:SceneBoard'], "Brief oral découpé en scènes", async ({ page }) => {
  const board = fx('SceneBoard');
  const entities = fx('entities');
  const media = fx('media');
  const nameOf = (id: string): string =>
    [...entities.characters, ...entities.places, ...entities.events].find((e: any) => e.id === id)?.name
    ?? [...media.battlemaps, ...media.illustrations].find((m: any) => m.id === id)?.title;
  await open(page, 'preparation-seance');
  const scenes = component(page, 'SceneBoard');
  const headings = scenes.getByRole('heading', { level: 3 });
  const card = (title: string) => scenes.getByRole('listitem').filter({ has: page.getByRole('heading', { name: title, exact: true }) });

  // The session's board before the brief: its scenes, in their order (possibly none). The board paints
  // its counter and its list together, so the counter says the list is there.
  await expect(scenes.locator('#scenes-present-count')).toHaveText(/\d+ sur \d+/);
  const before = (await headings.allTextContents()).map((t) => t.trim());

  // The brief is told, then processed.
  const oral = component(page, 'OralBrief');
  await oral.getByRole('button', { name: /Brief oral/ }).click();
  await oral.getByRole('button', { name: 'Terminer et analyser' }).click();
  // The brief shows the mock-up's result while it is analysed; the analysis is over once it says so.
  await expect(oral.getByText(/Brief analysé/)).toBeVisible({ timeout: 20_000 });

  // The board holds the scenes the brief was cut into once the screen is opened again,
  // after those it already held, which keep their order.
  await open(page, 'preparation-seance');
  await expect.poll(() => headings.count()).toBeGreaterThan(before.length);
  const titles = (await headings.allTextContents()).map((t) => t.trim());
  expect(titles.slice(0, before.length)).toEqual(before);

  // An ordered list of scenes, each linked to its places, NPCs, events and media: the mock-up's scenes
  // to exactly what it links, the others to at least one place, NPC, event or medium. Only the card's
  // link chips count: its title, its notes and its battlemap select can name an entity without any link.
  for (const title of titles) {
    const mocked = board.scenes.find((s: any) => s.title === title);
    if (mocked) {
      for (const id of [...mocked.places, ...mocked.characters, ...mocked.events, ...mocked.media]) {
        await expect(card(title)).toContainText(nameOf(id));
      }
    } else {
      // The « À créer » chips (`chip--todo`) are entities still to create, not links to existing ones.
      await expect(card(title).locator('.mf-scenes__links a:not(.chip--todo)').first(), `${title} is linked to nothing`).toBeVisible();
    }
  }

  // Reordered by hand: the second scene goes up.
  await scenes.getByRole('button', { name: 'Monter' }).nth(1).click();
  await expect(headings).toHaveText([titles[1], titles[0], ...titles.slice(2)]);

  // Completed by hand: a scene added, notes written.
  await scenes.getByRole('button', { name: 'Ajouter une scène' }).click();
  await expect(headings).toHaveCount(titles.length + 1);
  const notes = scenes.getByLabel('Notes').first();
  await notes.fill('Une note écrite à la main.');
  await expect(notes).toHaveValue('Une note écrite à la main.');
});
