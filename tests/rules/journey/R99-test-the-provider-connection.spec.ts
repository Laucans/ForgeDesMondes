import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: le DM a renseigné l'URL de son endpoint opencode maison, un modèle de génération et un modèle de résumé
// when: il clique sur « Tester la connexion »
// then: l'écran indique si la connexion réussit ou, sinon, la raison de l'échec, sans enregistrer de réglage invalide à son insu
rule('R99', ['screen:parametres-ia', 'micro-ui:AiSettings'], "Tester la connexion au fournisseur", async ({ page }) => {
  const settings = fx('AiSettings');
  const home = settings.providers.opencode;
  await open(page, 'parametres-ia');
  const ai = component(page, 'AiSettings');
  await ai.getByRole('radio', { name: /^API/ }).check();

  // The URL of the home endpoint and both models: one click tells whether it answers.
  await ai.getByLabel('URL de l’API').fill(home.url);
  await ai.getByLabel('Modèle de génération').fill(home.generationModel);
  await ai.getByLabel('Modèle de résumé').fill(home.summaryModel);
  // Own precondition: the home API is the saved provider (the runtime has one database, another journey may have saved a binary),
  // so a bad URL kept behind by the test would show in the form after the reload.
  await ai.getByRole('button', { name: 'Enregistrer' }).click();
  await ai.getByRole('button', { name: 'Tester la connexion' }).click();
  await expect(ai.getByText(/Connexion réussie/)).toBeVisible();

  // An URL that answers no known API: the reason of the failure is said, and nothing invalid is kept behind.
  await ai.getByLabel('URL de l’API').fill('https://inconnu.example.org/pas-une-api');
  await ai.getByRole('button', { name: 'Tester la connexion' }).click();
  await expect(ai.getByText(/reconnu|échou|refus|invalide|injoignable/i).first()).toBeVisible();
  await expect(ai.getByText(/Connexion réussie/)).toBeHidden();
  await page.reload();
  // The saved settings come back: the home URL proves they are loaded, so the bad one is not hidden by a first paint.
  await expect(component(page, 'AiSettings').getByLabel('URL de l’API')).toHaveValue(home.url);
  await expect(component(page, 'AiSettings').getByLabel('URL de l’API')).not.toHaveValue(/inconnu\.example\.org/);
});
