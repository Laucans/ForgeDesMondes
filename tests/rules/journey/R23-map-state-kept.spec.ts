import { expect, type Page } from '@playwright/test';
import { rule } from './kit';
import { component, open } from './helpers';

// given: un combat en cours sur une battlemap
// when: l'application est fermée puis rouverte
// then: positions, PV, états et initiative sont identiques, et « Réinitialiser la carte » vide la carte : plus aucun token ni ordre d'initiative, la carte et sa grille restent affichées.
rule('R23', ['screen:dm-view', 'micro-ui:Battlemap'], "État de la carte conservé", async ({ page, browser }) => {
  const reach = async (scope: Page) => {
    await open(scope, 'dm-view');
    await scope.getByRole('tab', { name: 'Battlemap' }).click();
    return component(scope, 'Battlemap');
  };
  const token = (map: ReturnType<typeof component>, name: string) => map.locator('.kit-token__item').filter({ hasText: name }).first();
  const position = async (scope: Page, name: string) => {
    const [v, d] = [await scope.locator('.hm-view').first().boundingBox(), await token(component(scope, 'Battlemap'), name).boundingBox()];
    return v && d ? { x: Math.round(d.x - v.x), y: Math.round(d.y - v.y) } : null;
  };

  // A combat is under way: Elowen moved, Bandit 2 hurt and frightened.
  const map = await reach(page);
  const start = { elowen: await position(page, 'Elowen'), bandit: await position(page, 'Bandit 2') };
  const disc = await token(map, 'Elowen').boundingBox();
  const at = { x: (disc?.x ?? 0) + (disc?.width ?? 0) / 2, y: (disc?.y ?? 0) + (disc?.height ?? 0) / 2 };
  await page.mouse.move(at.x, at.y);
  await page.mouse.down();
  await page.mouse.move(at.x + 90, at.y + 30, { steps: 6 });
  await page.mouse.up();
  await map.getByRole('button', { name: 'Lancer le combat' }).click();
  await page.getByRole('button', { name: 'Commencer le combat' }).click();
  const carousel = map.getByLabel('Ordre d’initiative');
  await expect(carousel).toBeVisible();
  await token(map, 'Bandit 2').click();
  const sheet = map.getByRole('complementary', { name: 'Bandit 2' });
  await sheet.getByLabel('Points de vie').fill('7');
  await sheet.getByRole('button', { name: /Dégâts/ }).click();
  await sheet.getByRole('button', { name: /Effrayé/ }).click();
  await expect(sheet.getByText(/\d+ \/ \d+ PV/)).toContainText('4 / 11');
  const kept = {
    elowen: await position(page, 'Elowen'),
    bandit: await position(page, 'Bandit 2'),
    initiative: (await carousel.locator('.kit-token__item').allInnerTexts()).join('|'),
  };
  expect(kept.elowen, 'Elowen was moved').not.toEqual(start.elowen);

  // The application is closed, then reopened in a context of its own: nothing of the first one is left.
  await page.context().close();
  const reopened = await browser.newContext();
  const again = await reopened.newPage();
  const map2 = await reach(again);
  await expect(map2.getByLabel('Ordre d’initiative')).toBeVisible();
  expect(await position(again, 'Elowen')).toEqual(kept.elowen);
  expect(await position(again, 'Bandit 2')).toEqual(kept.bandit);
  expect((await map2.getByLabel('Ordre d’initiative').locator('.kit-token__item').allInnerTexts()).join('|')).toBe(kept.initiative);
  await token(map2, 'Bandit 2').click();
  const sheet2 = map2.getByRole('complementary', { name: 'Bandit 2' });
  await expect(sheet2.getByText(/\d+ \/ \d+ PV/)).toContainText('4 / 11');
  await expect(sheet2.getByRole('button', { name: /Effrayé/ })).toHaveClass(/is-on/);

  // « Réinitialiser la carte » empties the map: no token, no initiative order, the map and its grid stay.
  await map2.getByRole('button', { name: 'Réinitialiser la carte' }).click();
  await again.getByRole('button', { name: 'Réinitialiser', exact: true }).click();
  await expect(map2.getByLabel('Ordre d’initiative')).toBeHidden();
  await expect(map2.locator('.kit-token__item')).toHaveCount(0);
  await expect(map2.locator('.hm-view').first()).toBeVisible();
  await expect(map2.locator('.hm-grid-path').first()).toHaveAttribute('d', /\S/);
  await reopened.close();
});
