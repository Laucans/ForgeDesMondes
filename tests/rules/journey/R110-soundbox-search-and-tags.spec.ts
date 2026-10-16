import { expect } from '@playwright/test';
import { rule } from './kit';
import { component, fx, open } from './helpers';

// given: une soundbox qui contient « porte qui grince » (tag porte) et « bruit de foule » (tag taverne)
// when: le DM tape « gri » dans la recherche, puis l'efface et filtre sur le tag taverne
// then: seule la case « porte qui grince » reste pendant la saisie, puis seule la case « bruit de foule » reste avec le filtre
rule('R110', ['screen:soundbox', 'screen:dm-view', 'micro-ui:SoundLibrary', 'micro-ui:LiveSoundbox'], "Recherche et tags dans la soundbox", async ({ page }) => {
  // The soundbox holds « porte qui grince » (tag porte) and « bruit de foule » (tag taverne) only.
  await open(page, 'soundbox');
  const library = component(page, 'SoundLibrary');
  const keep = ['Porte qui grince', 'Bruit de foule en taverne'];
  for (const sound of fx('sounds').sounds) {
    if (keep.includes(sound.name)) continue;
    await library.getByRole('row').filter({ hasText: sound.name }).getByRole('button', { name: 'Modifier' }).click();
    await library.getByRole('button', { name: 'Supprimer' }).click();
  }
  await expect(library.getByRole('row').filter({ hasText: '#' })).toHaveCount(keep.length);

  await open(page, 'dm-view');
  await page.getByRole('tab', { name: 'Soundbox' }).click();
  const box = component(page, 'LiveSoundbox');
  const door = box.getByRole('button', { name: /Porte qui grince/ });
  const crowd = box.getByRole('button', { name: /Bruit de foule/ });

  // « gri »: only the door remains while typing.
  await box.getByLabel('Rechercher un son').fill('gri');
  await expect(door).toBeVisible();
  await expect(crowd).toBeHidden();

  // The search cleared, the tag « taverne »: only the crowd remains.
  await box.getByLabel('Rechercher un son').fill('');
  await box.getByRole('button', { name: '#taverne' }).click();
  await expect(crowd).toBeVisible();
  await expect(door).toBeHidden();
});
