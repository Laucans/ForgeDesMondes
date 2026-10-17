import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: le micro a détecté successivement la taverne, le tavernier puis Morlox
// when: le DM clique sur l'icône d'historique de la pastille puis sur la taverne
// then: l'historique liste ces entités de la plus récente à la plus ancienne, et la fiche de la taverne s'ouvre au centre
rule('R109', ['screen:dm-view', 'micro-ui:SuggestionPill', 'micro-ui:TableSheet'], "Historique des suggestions", async ({ page }) => {
  await open(page, 'dm-view');
  const pill = component(page, 'SuggestionPill').locator('aside');
  const mic = pill.locator('#suggestions-mic').getByRole('button').first();
  const entities = pill.locator('#suggestions-stack button[data-kind="entity"] b');

  // The three entities of the given, named as the mock-up's entities name them.
  const feed = fx('SuggestionPill').feed;
  const nameOf = (id: string): string => feed.find((s: { id: string }) => s.id === id).name;
  const tavern = nameOf('taverne-sanglier');
  const keeper = nameOf('gaspard');
  const morlox = nameOf('morlox');

  // The microphone runs until it has detected all three; the order of arrival of the entities is noted.
  await mic.click();
  const arrived: string[] = [];
  await expect
    .poll(async () => {
      const latest = (await entities.count()) ? (await entities.first().innerText()).trim() : '';
      if (latest && arrived[arrived.length - 1] !== latest) arrived.push(latest);
      return arrived.includes(morlox);
    }, { timeout: 60_000, intervals: [200] })
    .toBe(true);
  await mic.click();

  // The given holds: the tavern, then the keeper, then Morlox (other entries of the feed may sit between them).
  expect(arrived.indexOf(tavern)).toBeGreaterThanOrEqual(0);
  expect(arrived.indexOf(keeper)).toBeGreaterThan(arrived.indexOf(tavern));
  expect(arrived.indexOf(morlox)).toBeGreaterThan(arrived.indexOf(keeper));

  // The history icon lists the three from the latest to the oldest.
  await pill.getByRole('button', { name: 'Historique des 5 dernières entités' }).click();
  const history = pill.getByRole('region', { name: '5 dernières entités' }).locator('li b');
  const listed = (await history.allInnerTexts()).map((t) => t.trim());
  expect(listed.filter((name) => [tavern, keeper, morlox].includes(name))).toEqual([morlox, keeper, tavern]);

  // A click on the tavern, by name, opens its sheet in the centre.
  await history.filter({ hasText: tavern }).click();
  await expect(component(page, 'TableSheet').getByRole('heading', { name: tavern })).toBeVisible();
});
