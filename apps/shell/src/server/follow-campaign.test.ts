// The four DM screens that follow the campaign, as the repository freezes them:
// the real files, the real contract.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Context } from '../client/context.js';
import { repositoryRoot } from './config.js';
import { contractValidator, loadScreen } from './screens.js';

const root = repositoryRoot();

// AC1: every campaign prop of soundbox, campagne, fiche-entite and medias is "$ctx.campaign", each version is one higher, each keeps its surface, session, instance and entity props are unchanged
test('the four screens read $ctx.campaign, at version 2, on the dm surface, their other props as frozen', async () => {
  const validate = await contractValidator(root);
  const layoutOf = async (name: string) => {
    const screen = await loadScreen(root, name, validate);
    assert.ok(screen, `${name} breaks no contract`);
    assert.equal(screen.version, 2, name);
    assert.equal(screen.surface, 'dm', name);
    return screen.layout;
  };
  const follow = { campaign: '$ctx.campaign' };

  assert.deepEqual(await layoutOf('soundbox'), [
    { microUi: 'SoundLibrary', props: follow },
    { microUi: 'LiveSoundbox', props: {} },
  ]);
  assert.deepEqual(await layoutOf('campagne'), [
    { microUi: 'CampaignOverview', props: follow },
    { microUi: 'SessionList', props: follow },
    { microUi: 'CampaignElements', props: { mode: 'tabs', ...follow } },
    { microUi: 'ToCreateList', props: follow },
    { microUi: 'NoteImport', props: follow },
  ]);
  assert.deepEqual(await layoutOf('fiche-entite'), [
    { microUi: 'CampaignElements', props: { mode: 'rail', ...follow } },
    { microUi: 'EntityEditor', props: {} },
    { microUi: 'VersionHistory', props: { entity: '$ctx.entity' } },
  ]);
  // The spec has medias keep session s4 (number 4) whatever campaign is shown: pinned as specified, not as a wish.
  assert.deepEqual(await layoutOf('medias'), [
    { microUi: 'MediaLibrary', props: { ...follow, session: 's4', sessionNumber: 4 } },
    { microUi: 'GridSettings', props: { battlemap: '$ctx.battlemap' } },
  ]);
});

// The follow props are resolved from the address's campaign; the other props stay as frozen.
test("the address's campaign reaches every campaign prop of the four screens, and nothing else", async () => {
  const validate = await contractValidator(root);
  const context = new Context();
  context.hold('campaign', 'ember');
  for (const name of ['soundbox', 'campagne', 'fiche-entite', 'medias']) {
    const screen = await loadScreen(root, name, validate);
    assert.ok(screen, name);
    for (const { microUi, props } of screen.layout) {
      const resolved = context.resolve(props);
      assert.deepEqual(Object.keys(resolved), Object.keys(props), `${name} ${microUi}`);
      for (const [key, value] of Object.entries(props)) {
        if (value === '$ctx.campaign') assert.equal(resolved[key], 'ember', `${name} ${microUi}.${key}`);
        else if (typeof value !== 'string' || !value.startsWith('$ctx.')) assert.deepEqual(resolved[key], value, `${name} ${microUi}.${key}`);
      }
    }
  }
});
