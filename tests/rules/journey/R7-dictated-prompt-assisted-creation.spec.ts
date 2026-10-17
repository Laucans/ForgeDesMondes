import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un prompt initial dicté pour un PNJ
// when: le DM demande la proposition, successivement avec une API configurée et avec un binaire configuré
// then: un background cohérent avec le prompt et un statblock jouable sont proposés dans chacun des deux modes
rule('R7', ['screen:parametres-ia', 'screen:fiche-entite', 'micro-ui:AiSettings', 'micro-ui:EntityEditor'], "Prompt initial dicté et création assistée", async ({ page }) => {
  const settings = fx('AiSettings');
  const dictation: string = fx('EntityEditor').dictations.default;
  const words = (text: string): string[] => text.toLowerCase().match(/[a-zàâçéèêëîïôûùüÿœ]{5,}/g) ?? [];
  for (const mode of ['api', 'binary']) {
    // The provider configured: an API by its URL, then a binary.
    await open(page, 'parametres-ia');
    const ai = component(page, 'AiSettings');
    if (mode === 'api') {
      await ai.getByRole('radio', { name: /^API/ }).check();
      await ai.getByLabel('URL de l’API').fill(settings.providers.opencode.url);
    } else {
      await ai.getByRole('radio', { name: /^Binaire/ }).check();
      await ai.getByRole('combobox', { name: 'Binaire' }).selectOption({ label: 'Claude Code' });
    }
    await ai.getByRole('button', { name: 'Enregistrer' }).click();

    // A NPC, its initial prompt dictated, the proposal asked.
    await open(page, 'fiche-entite', '?new=pnj');
    const editor = component(page, 'EntityEditor');
    await editor.getByLabel('Prompt initial').fill(dictation);
    await editor.getByRole('button', { name: /Proposer background, comportement et statblock/ }).click();

    const background = editor.getByLabel('Background');
    await expect(background).not.toHaveValue('', { timeout: 15_000 });
    const proposed = await background.inputValue();
    expect(words(proposed).some((w) => words(dictation).includes(w)), `${mode}: the background shares nothing with the prompt`).toBe(true);
    // A statblock playable as it is: abilities, AC, HP and at least one ability.
    for (const stat of ['FOR', 'DEX', 'CON', 'INT', 'SAG', 'CHA']) {
      await expect(editor.getByLabel(new RegExp(`^${stat}`))).toHaveValue(/^\d+$/);
    }
    expect(Number(await editor.getByLabel('CA', { exact: true }).inputValue())).toBeGreaterThan(0);
    expect(Number(await editor.getByLabel('PV max').inputValue())).toBeGreaterThan(0);
    await expect(editor.getByRole('button', { name: /^Retirer / }).first()).toBeVisible();
  }
});
