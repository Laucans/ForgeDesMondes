// The frozen `campagnes` screen as the repository holds it: the real file, the
// real contract, the real Micro-UI manifest and the real mock-up listing.

import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { repositoryRoot } from './config.js';
import { contractValidator, loadScreen } from './screens.js';

const root = repositoryRoot();
const json = async (path: string): Promise<unknown> => JSON.parse(await readFile(join(root, path), 'utf8'));

// AC1: apps/screens/campagnes.json is valid against contract E and its layout composes CampaignList
test('the campagnes screen breaks no contract and lays out CampaignList alone', async () => {
  const screen = await loadScreen(root, 'campagnes', await contractValidator(root));
  assert.deepEqual(screen?.layout, [{ microUi: 'CampaignList', props: {} }]);
});

// AC2: every Micro-UI the frozen screen names resolves to a built one
test('the Micro-UI the campagnes screen lays out is a built one, of the campaign system', async () => {
  const screen = await loadScreen(root, 'campagnes', await contractValidator(root));
  const manifest = (await json('apps/campaign/campaign-list/micro-ui.json')) as { microUi: string; system: string };
  assert.deepEqual(screen?.layout.map((placement) => placement.microUi), [manifest.microUi]);
  assert.equal(manifest.system, 'campaign');
});

// AC4: pages/campagnes.html stays mocked: mock/pages.json lists it for the screen, and it injects what the screen lays out
test('the campagnes screen is mocked by pages/campagnes.html, which injects CampaignList', async () => {
  const { pages } = (await json('mock/pages.json')) as { pages: Array<{ path: string; screen?: string; components: string[] }> };
  const page = pages.find((candidate) => candidate.screen === 'campagnes');
  assert.equal(page?.path, 'pages/campagnes.html');
  assert.deepEqual(page?.components, ['CampaignList']);
  await access(join(root, 'mock', page.path));
  assert.match(await readFile(join(root, 'mock', page.path), 'utf8'), /data-component="CampaignList"/);
});
