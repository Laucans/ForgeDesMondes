import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: le micro a détecté successivement la taverne, Morlox et le tavernier
// when: le DM clique sur l'icône d'historique de la pastille puis sur la taverne
// then: l'historique liste ces entités de la plus récente à la plus ancienne, et la fiche de la taverne s'ouvre au centre
rule('R109', ['screen:dm-view', 'micro-ui:SuggestionPill', 'micro-ui:TableSheet'], "Historique des suggestions", async ({ page }) => {
  await open(page, 'dm-view');
  const pill = component(page, 'SuggestionPill').locator('aside');
  const mic = pill.locator('#suggestions-mic').getByRole('button').first();
  const entities = pill.locator('#suggestions-stack button[data-kind="entity"] b');

  // The microphone detects three entities one after the other; their order of arrival is noted.
  await mic.click();
  const arrived: string[] = [];
  await expect
    .poll(async () => {
      const latest = (await entities.count()) ? (await entities.first().innerText()).trim() : '';
      if (latest && arrived[arrived.length - 1] !== latest) arrived.push(latest);
      return arrived.length;
    }, { timeout: 60_000, intervals: [200] })
    .toBeGreaterThanOrEqual(3);
  await mic.click();

  // The history icon lists them, the latest first; the first one opens in the centre.
  await pill.getByRole('button', { name: 'Historique des 5 dernières entités' }).click();
  const history = pill.getByRole('region', { name: '5 dernières entités' }).locator('li b');
  const listed = (await history.allInnerTexts()).map((t) => t.trim());
  expect(listed.slice(0, arrived.length)).toEqual([...arrived].reverse().slice(0, 5));
  const first = arrived[0];
  await history.filter({ hasText: first }).click();
  await expect(component(page, 'TableSheet').getByRole('heading', { name: first })).toBeVisible();
});
