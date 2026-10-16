import { expect, type Page } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une battlemap envoyée aux joueurs et un onglet player view ouvert par le DM
// when: il y déplace un token, zoome et décale le cadrage
// then: le token est à la nouvelle position dans la DM view, et le zoome et le cadrage sont les mêmes sur la player view projetée
rule('R113', ['screen:dm-view', 'screen:player-view', 'micro-ui:PlayerStage', 'micro-ui:Battlemap'], "Onglet player view piloté par le DM", async ({ page, context }) => {
  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Battlemap' }).click();
  const dmMap = component(page, 'Battlemap');

  // The battlemap is sent; the DM's tab and the projected player view open.
  await dmMap.getByRole('button', { name: 'Envoyer aux joueurs' }).click();
  const pilot = await context.newPage();
  await open(pilot, 'player-view', '?pilote=1');
  const projected = await context.newPage();
  await open(projected, 'player-view');

  const stage = (scope: Page) => scope.locator('.hm-stage').first();
  const relative = async (scope: Page, name: string) => {
    const [v, d] = [await scope.locator('.hm-view').first().boundingBox(), await scope.locator('.kit-token__item').filter({ hasText: name }).first().boundingBox()];
    return v && d ? { x: (d.x - v.x) / v.width, y: (d.y - v.y) / v.height } : null;
  };
  const before = await relative(page, 'Elowen');

  // In the pilot tab: a token moves, the map zooms and is shifted.
  const disc = await pilot.locator('.kit-token__item').filter({ hasText: 'Elowen' }).first().boundingBox();
  const at = { x: (disc?.x ?? 0) + (disc?.width ?? 0) / 2, y: (disc?.y ?? 0) + (disc?.height ?? 0) / 2 };
  await pilot.mouse.move(at.x, at.y);
  await pilot.mouse.down();
  await pilot.mouse.move(at.x + 90, at.y + 30, { steps: 6 });
  await pilot.mouse.up();
  await pilot.getByRole('button', { name: 'Zoomer' }).click();
  await pilot.locator('.hm-view').first().dispatchEvent('wheel', { deltaY: -120 });
  const view = await pilot.locator('.hm-view').first().boundingBox();
  await pilot.mouse.move((view?.x ?? 0) + 40, (view?.y ?? 0) + 40);
  await pilot.mouse.down();
  await pilot.mouse.move((view?.x ?? 0) + 100, (view?.y ?? 0) + 70, { steps: 6 });
  await pilot.mouse.up();

  // The token is elsewhere in the DM view; zoom and framing are the same on the projected view.
  await page.bringToFront();
  await expect.poll(async () => JSON.stringify(await relative(page, 'Elowen'))).not.toBe(JSON.stringify(before));
  await expect
    .poll(async () => (await stage(projected).getAttribute('style')) === (await stage(pilot).getAttribute('style')))
    .toBe(true);
});
