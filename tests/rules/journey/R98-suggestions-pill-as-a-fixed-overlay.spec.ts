import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une séance en direct dans la DM view, avec le micro en marche et une entité déjà suggérée dans la pastille
// when: le DM ouvre au centre une longue fiche en lecture et la fait défiler jusqu'en bas, puis ouvre la battlemap de la scène et enfin l'onglet Soundbox
// then: la pastille et le bouton marche/pause du micro restent affichés au même endroit en bas à droite de la fenêtre, par-dessus le contenu, sans le décaler, et un clic sur l'entité suggérée l'ouvre toujours au centre
rule('R98', ['screen:dm-view', 'micro-ui:SuggestionPill', 'micro-ui:ElementsPanel', 'micro-ui:TableSheet', 'micro-ui:Battlemap', 'micro-ui:LiveSoundbox'], "Pastille des suggestions en surcouche fixe", async ({ page }) => {
  await open(page, 'dm-view');
  const pill = component(page, 'SuggestionPill').locator('aside');
  const mic = pill.locator('#suggestions-mic').getByRole('button').first();

  // The microphone runs and an entity is suggested.
  await mic.click();
  const suggested = pill.locator('#suggestions-stack button[data-kind="entity"]').first();
  await expect(suggested).toBeVisible({ timeout: 20_000 });
  const name = (await suggested.locator('b').innerText()).trim();

  const place = async () => {
    const [p, m] = [await pill.boundingBox(), await mic.boundingBox()];
    const view = page.viewportSize() ?? { width: 0, height: 0 };
    return { pill: p, mic: m, view };
  };
  const rest = await place();
  const sameAs = async (what: string) => {
    const now = await place();
    for (const k of ['pill', 'mic'] as const) {
      for (const side of ['x', 'y', 'width', 'height'] as const) {
        expect(Math.abs((now[k]?.[side] ?? NaN) - (rest[k]?.[side] ?? NaN)), `${what}: the ${k}'s ${side} moved`).toBeLessThan(2);
      }
    }
  };
  // Anchored at the bottom right of the window, above the page.
  expect((rest.pill?.x ?? 0) + (rest.pill?.width ?? 0)).toBeGreaterThan(rest.view.width * 0.8);
  expect((rest.pill?.y ?? 0) + (rest.pill?.height ?? 0)).toBeGreaterThan(rest.view.height * 0.8);
  expect(await pill.evaluate((el) => getComputedStyle(el).position)).toBe('fixed');

  // A long sheet is read down to its bottom, the left panel scrolled too.
  await component(page, 'ElementsPanel').getByText('Vesna la Balafrée').first().click();
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await component(page, 'ElementsPanel').evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await sameAs('the long sheet');

  // The battlemap, then the Soundbox tab.
  await page.getByRole('tab', { name: 'Battlemap' }).click();
  await sameAs('the battlemap');
  await page.getByRole('tab', { name: 'Soundbox' }).click();
  await sameAs('the soundbox');

  // The pill sits over the content without moving it, and stays clickable.
  const centre = page.getByRole('main');
  const withPill = await centre.boundingBox();
  await pill.evaluate((el) => { (el as HTMLElement).style.visibility = 'hidden'; });
  const withoutPill = await centre.boundingBox();
  await pill.evaluate((el) => { (el as HTMLElement).style.visibility = ''; });
  expect(withPill).toEqual(withoutPill);
  await suggested.click();
  await expect(component(page, 'TableSheet').getByRole('heading', { name })).toBeVisible();
});
