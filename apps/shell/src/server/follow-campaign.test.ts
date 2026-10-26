// The four DM screens that follow the campaign, as the repository freezes them:
// the real files, the real contract.

import assert from 'node:assert/strict';
import { test } from 'node:test';
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
  // medias keeps the session cendres-valombre's fourth, whatever campaign is shown.
  assert.deepEqual(await layoutOf('medias'), [
    { microUi: 'MediaLibrary', props: { ...follow, session: 's4', sessionNumber: 4 } },
    { microUi: 'GridSettings', props: { battlemap: '$ctx.battlemap' } },
  ]);
});
