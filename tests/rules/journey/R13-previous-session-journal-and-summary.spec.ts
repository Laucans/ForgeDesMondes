import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une séance précédente close
// when: le DM prépare la suivante
// then: son résumé et son journal horodaté sont consultables dans le panneau latéral et le résumé figure dans la liste des séances.
rule('R13', ['screen:preparation-seance', 'screen:campagne', 'micro-ui:PreviousSummary', 'micro-ui:SessionList'], "Journal et résumé de la séance précédente", async ({ page }) => {
  const previous = fx('PreviousSummary');
  const sessions = fx('SessionList');
  await open(page, 'preparation-seance');
  const panel = component(page, 'PreviousSummary');
  await expect(panel).toContainText(previous.summary);

  // The timestamped journal opens in the side panel.
  await panel.getByRole('button', { name: 'Journal complet' }).click();
  const journal = panel.getByRole('complementary', { name: 'Journal transcrit' });
  await expect(journal).toBeVisible();
  for (const line of previous.journal) {
    const row = journal.getByRole('listitem').filter({ hasText: line.text });
    await expect(row).toContainText(line.t);
  }

  // The summary is in the list of the sessions too.
  await open(page, 'campagne');
  const list = component(page, 'SessionList');
  for (const played of sessions.sessions.filter((s: any) => s.status === 'played')) {
    await expect(list).toContainText(played.summary);
  }
});
