import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: un PNJ manquant dans la liste « à créer », issu du brief
// when: le DM choisit « Affiner », dicte trois phrases de plus et valide
// then: la proposition de l'IA (background et statblock) tient compte à la fois de l'extrait du brief et des phrases ajoutées, et l'entité quitte la liste « à créer » une fois créée
rule('R103', ['screen:campagne', 'micro-ui:ToCreateList'], "Affiner une entité manquante", async ({ page }) => {
  const wanted = fx('ToCreateList');
  const odric = wanted.items.find((i: any) => i.id === 'tc-odric');
  const dictated: string = wanted.proposals['tc-odric'].dictation;
  const words = (text: string): string[] => text.toLowerCase().match(/[a-zàâçéèêëîïôûùüÿœ]{5,}/g) ?? [];
  await open(page, 'campagne');
  const list = component(page, 'ToCreateList');
  const item = list.getByRole('listitem').filter({ has: page.getByRole('heading', { name: odric.name }) });
  await expect(item.getByText('À créer')).toBeVisible();
  const remaining = async () => Number((await list.getByText(/à créer ·/).innerText()).match(/^(\d+)/)?.[1]);
  const before = await remaining();

  // « Affiner »: three more sentences are dictated, then validated.
  await item.getByRole('button', { name: 'Affiner' }).click();
  await item.getByLabel('Description affinée').fill(dictated);
  await item.getByRole('button', { name: 'Proposer' }).click();

  // The proposal holds what the brief said and what was added.
  const proposal = await item.locator('dl').innerText();
  const fromBrief = words(odric.excerpt);
  const added = words(dictated).filter((w) => !fromBrief.includes(w));
  expect(words(proposal).some((w) => fromBrief.includes(w)), 'nothing of the brief excerpt').toBe(true);
  expect(words(proposal).some((w) => added.includes(w)), 'nothing of the added sentences').toBe(true);
  await expect(item.getByText('Background')).toBeVisible();
  await expect(item.getByText('CA', { exact: true })).toBeVisible();

  // Created, the entity leaves the list of those to create.
  await expect(item.getByText('Créé')).toBeVisible();
  expect(await remaining()).toBe(before - 1);
});
