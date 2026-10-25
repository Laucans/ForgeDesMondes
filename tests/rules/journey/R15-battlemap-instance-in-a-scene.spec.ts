import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une scène liée à une battlemap
// when: le DM y place à l'avance PJ, PNJ et monstres
// then: l'instance de la scène les conserve à leur position, sans modifier la battlemap d'origine.
rule('R15', ['screen:preparation-seance', 'screen:medias', 'micro-ui:SceneBoard', 'micro-ui:Battlemap', 'micro-ui:MediaLibrary', 'micro-ui:GridSettings'], "Instance de battlemap dans une scène", async ({ page }) => {
  const media = fx('media');
  const original = media.battlemaps.find((b: any) => b.id === 'bm-gue');
  await open(page, 'preparation-seance');

  // The scene's battlemap instance: a PJ and a monster placed ahead of time.
  const map = component(page, 'Battlemap');
  await expect(map).toBeVisible();
  // A token's place in the map: its offset from the map's own box, not its viewport coordinates,
  // which move with the page's scroll (the map sits where the screen's layout puts it).
  const placed = async (name: string): Promise<{ x: number; y: number }> => {
    const token = await component(page, 'Battlemap').locator('.kit-token__item').filter({ hasText: name }).first().boundingBox();
    const frame = await component(page, 'Battlemap').boundingBox();
    return { x: (token?.x ?? NaN) - (frame?.x ?? NaN), y: (token?.y ?? NaN) - (frame?.y ?? NaN) };
  };
  const roster = map.getByRole('complementary', { name: 'Personnages de la scène' });
  for (const [who, count] of [['Elowen', 1], ['Bandit', 2]] as const) {
    await roster.getByRole('button', { name: new RegExp(who) }).click();
    const drop = map.locator('[data-no-pan]').filter({ hasText: `Placer ${who}` });
    if (count > 1) await drop.getByRole('spinbutton').fill(String(count));
    await drop.getByRole('button', { name: 'Placer' }).click();
  }
  const before = { elowen: await placed('Elowen'), bandit: await placed('Bandit 2') };

  // The instance keeps them where they are once the page is opened again.
  await open(page, 'preparation-seance');
  const again = component(page, 'Battlemap');
  await expect(again.locator('.kit-token__item').filter({ hasText: 'Bandit 2' })).toBeVisible();
  for (const [who, was] of [['Elowen', before.elowen], ['Bandit 2', before.bandit]] as const) {
    const now = await placed(who);
    expect(Math.abs(now.x - was.x)).toBeLessThan(1);
    expect(Math.abs(now.y - was.y)).toBeLessThan(1);
  }

  // The battlemap of the library is as it was: same grid, no token.
  await open(page, 'medias');
  const library = component(page, 'MediaLibrary');
  await library.getByRole('button', { name: 'Régler la grille' }).first().click();
  const grid = component(page, 'GridSettings');
  await expect(grid.getByLabel('Taille d’un hex')).toHaveValue(String(original.grid.size));
  await expect(grid.locator('.kit-token__item')).toHaveCount(0);
});
