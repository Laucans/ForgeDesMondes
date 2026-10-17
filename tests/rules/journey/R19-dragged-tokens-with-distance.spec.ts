import { expect, type Page } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un token glissé de 6 hex
// when: le DM le déplace
// then: le compteur affiche « 6 hex · 30 pi », et une fois déposé le token apparaît au même endroit sur la player view en moins d'une seconde.
rule('R19', ['screen:dm-view', 'screen:player-view', 'micro-ui:Battlemap', 'micro-ui:PlayerStage'], "Tokens glissés avec mesure de distance", async ({ page, context }) => {
  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Battlemap' }).click();
  const map = component(page, 'Battlemap');
  const player = await context.newPage();
  await open(player, 'player-view');
  await page.bringToFront();

  // The cursor carries the token; the green counter follows it away from the starting hex.
  const token = map.locator('.kit-token__item').filter({ hasText: 'Elowen' }).first();
  const box = await token.boundingBox();
  const from = { x: (box?.x ?? 0) + (box?.width ?? 0) / 2, y: (box?.y ?? 0) + (box?.height ?? 0) / 2 };
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  const counter = page.getByText(/^\s*\d+ hex · \d+ pi\s*$/);
  let step = 0;
  while (step < 200) {
    step += 1;
    await page.mouse.move(from.x + step * 3, from.y);
    if ((await counter.count()) && /^\s*6 hex · 30 pi\s*$/.test(await counter.first().innerText())) break;
  }
  await expect(counter.first()).toHaveText(/^\s*6 hex · 30 pi\s*$/);
  await page.mouse.up();

  // Dropped, the token is at the same place on the player view within a second.
  const position = async (scope: Page): Promise<{ x: number; y: number } | null> => {
    const view = scope.locator('.hm-view').first();
    const disc = scope.locator('.kit-token__item').filter({ hasText: 'Elowen' }).first();
    const [v, d] = [await view.boundingBox(), await disc.boundingBox()];
    return v && d ? { x: (d.x - v.x) / v.width, y: (d.y - v.y) / v.height } : null;
  };
  await expect
    .poll(async () => {
      const [mine, theirs] = [await position(page), await position(player)];
      return !!mine && !!theirs && Math.abs(mine.x - theirs.x) < 0.02 && Math.abs(mine.y - theirs.y) < 0.02;
    }, { timeout: 1000 })
    .toBe(true);
});
