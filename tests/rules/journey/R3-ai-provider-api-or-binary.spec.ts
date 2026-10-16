import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: l'écran des paramètres IA, d'abord en mode « API » avec l'URL du serveur maison saisie, puis en mode « Binaire »
// when: le DM teste la connexion en mode « API », puis passe en mode « Binaire », ouvre la liste déroulante, choisit « Claude Code » et teste à nouveau la connexion
// then: le type d'API détecté et le résultat du premier test s'affichent, la liste déroulante propose exactement « opencode » et « Claude Code », le résultat du second test s'affiche, et le rappel « seul du texte part vers ce fournisseur » reste visible
rule('R3', ['screen:parametres-ia', 'micro-ui:AiSettings'], "Fournisseur de génération paramétrable : API ou binaire", async ({ page }) => {
  const settings = fx('AiSettings');
  const home = settings.providers.opencode;
  await open(page, 'parametres-ia');
  const ai = component(page, 'AiSettings');
  const reminder = ai.getByText(/seul du texte part vers le fournisseur/i);
  await expect(reminder).toBeVisible();

  // « API » mode, the home server's URL.
  await ai.getByRole('radio', { name: /^API/ }).check();
  await ai.getByLabel('URL de l’API').fill(home.url);
  await expect(ai.getByText(/Type d’API détecté/)).toContainText(home.label);
  await ai.getByRole('button', { name: 'Tester la connexion' }).click();
  const first = ai.getByText(/Connexion réussie/);
  await expect(first).toBeVisible();
  const firstResult = await first.innerText();
  await expect(reminder).toBeVisible();

  // « Binaire » mode: exactly two binaries, then Claude Code is tested.
  await ai.getByRole('radio', { name: /^Binaire/ }).check();
  const binary = ai.getByRole('combobox', { name: 'Binaire' });
  await expect(binary.getByRole('option')).toHaveText(['opencode', 'Claude Code']);
  await binary.selectOption({ label: 'Claude Code' });
  await ai.getByRole('button', { name: 'Tester la connexion' }).click();
  const second = ai.getByText(/Connexion réussie/);
  await expect(second).toBeVisible();
  await expect(second).not.toHaveText(firstResult);
  await expect(reminder).toBeVisible();
});
