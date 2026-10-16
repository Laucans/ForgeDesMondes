import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: rien d'envoyé
// when: le DM envoie une illustration en fenêtre flottante puis la redimensionne
// then: la player view passe de la couverture à l'illustration et garde ses proportions.
rule('R24', ['screen:dm-view', 'screen:player-view', 'micro-ui:LiveSessionBar', 'micro-ui:ElementsPanel', 'micro-ui:PlayerStage'], "Diffusion vers la player view", async ({ page, context }) => {
  const media = fx('media');
  const illustration = media.illustrations.find((i: any) => i.id === 'ill-gaspard');
  await open(page, 'dm-view');
  const player = await context.newPage();
  await open(player, 'player-view');

  // Nothing sent: the cover of the campaign.
  const stage = player.locator('[data-component="PlayerStage"]');
  await expect(stage.getByRole('heading', { level: 1 })).toBeVisible();
  const cover = stage.getByRole('img').first();
  await expect(cover).toBeVisible();

  // The DM sends an illustration as a floating window, then resizes it.
  await page.bringToFront();
  await component(page, 'ElementsPanel').getByText(illustration.title).first().click();
  await component(page, 'TableSheet').getByRole('button', { name: 'Envoyer — fenêtre' }).click();
  const floating = stage.locator('figure');
  await expect(floating).toBeVisible();
  const small = await floating.boundingBox();
  const handle = await floating.locator('[aria-hidden="true"]').last().boundingBox();
  await player.mouse.move((handle?.x ?? 0) + 2, (handle?.y ?? 0) + 2);
  await player.mouse.down();
  await player.mouse.move((handle?.x ?? 0) + 120, (handle?.y ?? 0) + 40, { steps: 8 });
  await player.mouse.up();
  const large = await floating.boundingBox();
  expect((large?.width ?? 0) > (small?.width ?? 0), 'the window grew').toBe(true);

  // The player view went from the cover to the illustration and kept its proportions.
  const ratio = (b: { width: number; height: number } | null) => (b ? b.width / b.height : NaN);
  expect(ratio(large)).toBeCloseTo(illustration.ratio, 1);
  expect(ratio(small)).toBeCloseTo(illustration.ratio, 1);
});
