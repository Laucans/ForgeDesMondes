import assert from 'node:assert/strict';
import { test } from 'node:test';
import { campaignId, commandId, countsOf, exportCommand, isFinal, lastExportText, nextSessionDay, parseCampaign, safeFileName, settled } from '../src/model.ts';

const answer = { id: 'c1', name: 'Les Cendres', type: 'campagne', art: 'ember', glyph: 'V', pitch: 'Un bourg.', lastExport: null, counts: { players: 4, characters: 9, places: 4, events: 3 } };

test('the answer of an unknown campaign (null) is no campaign', () => {
  assert.equal(parseCampaign(null), null);
  assert.equal(parseCampaign({ name: 'sans id' }), null);
});

test('a campaign keeps the counts it carries and no others', () => {
  const campaign = parseCampaign({ ...answer, counts: { players: 4, places: '3', events: 0 } });
  assert.deepEqual(countsOf(campaign!), [
    { value: 4, label: 'joueurs' },
    { value: 0, label: 'événements' },
  ]);
});

test('the counts come in the order the mock-up draws them', () => {
  const campaign = parseCampaign({ ...answer, counts: { sounds: 1, sessions: 2, media: 3, players: 4 } });
  assert.deepEqual(
    countsOf(campaign!).map((count) => count.label),
    ['séances', 'joueurs', 'médias', 'sons'],
  );
});

test('a oneshot is a oneshot, anything else a campaign; a next session needs both its fields', () => {
  assert.equal(parseCampaign({ ...answer, type: 'oneshot' })!.type, 'oneshot');
  assert.equal(parseCampaign({ ...answer, type: 'autre' })!.type, 'campagne');
  assert.equal(parseCampaign({ ...answer, nextSession: { order: 4 } })!.nextSession, null);
  assert.deepEqual(parseCampaign({ ...answer, nextSession: { order: 4, date: '2026-10-17' } })!.nextSession, { order: 4, date: '2026-10-17' });
});

test('the last export is a day in words, or none yet', () => {
  assert.equal(lastExportText(null), 'Jamais exporté');
  assert.equal(parseCampaign({ ...answer, lastExport: '' })!.lastExport, null);
  assert.match(lastExportText('2026-10-04'), /^Dernier export : 4 octobre 2026$/);
  assert.match(lastExportText('2026-10-04T23:59:00'), /octobre 2026$/);
  assert.equal(lastExportText('pas une date'), 'Dernier export : pas une date');
});

test('the next session day is written in French, whatever the time zone', () => {
  assert.equal(nextSessionDay('2026-10-17'), 'samedi 17 octobre');
});

test('the campaign opened is the prop, else the address', () => {
  assert.equal(campaignId({ campaign: ' c1 ' }, '?id=c2'), 'c1');
  assert.equal(campaignId({}, '?id=c2'), 'c2');
  assert.equal(campaignId({}, ''), '');
});

test('the export names its DataCapability and the campaign, and nothing else', () => {
  assert.deepEqual(exportCommand('campaign.exportCampaign', 'c1'), {
    dataCapability: 'campaign.exportCampaign@1',
    payload: { id: 'c1' },
    caller: { by: 'dm', from: 'CampaignOverview', roles: [] },
  });
});

test('a command answer is read for its id and its state', () => {
  assert.equal(commandId({ commandId: 'x' }), 'x');
  assert.equal(commandId({ result: { commandId: 'y' } }), 'y');
  assert.equal(commandId(null), '');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['a', 3] } }), { status: 'rejected', violations: ['a'], archive: null });
  assert.deepEqual(settled('nope'), { status: '', violations: [], archive: null });
  assert.equal(isFinal('applied') && isFinal('rejected') && isFinal('expired'), true);
  assert.equal(isFinal('queued') || isFinal('awaiting_confirmation'), false);
});

// AC1: the archive is the attachment of the result, with its file name and its content
test('an applied export carries its archive as the attachment; a file with no name or no content is none', () => {
  const archive = { file: 'les-cendres-2026-10-14T10-00-00Z.json', content: { format: 'campaign-archive' } };
  assert.deepEqual(settled({ commandId: 'c1', result: { status: 'applied', violations: [], attachment: archive } }), { status: 'applied', violations: [], archive });
  assert.equal(settled({ result: { status: 'queued', attachment: null } }).archive, null);
  assert.equal(settled({ result: { status: 'applied', attachment: { file: '', content: {} } } }).archive, null);
  assert.equal(settled({ result: { status: 'applied', attachment: { file: 'x.json' } } }).archive, null);
  assert.equal(settled({ result: { status: 'applied', attachment: { file: 'x.json', content: null } } }).archive, null);
});

// AC1: a safe file name
test('a file name keeps nothing that is a path', () => {
  assert.equal(safeFileName('les-cendres-2026-10-14T10-00-00Z.json'), 'les-cendres-2026-10-14T10-00-00Z.json');
  assert.equal(safeFileName('../../etc/passwd'), '-..-etc-passwd');
  assert.equal(safeFileName('a:b*c.json'), 'a-b-c.json');
  assert.equal(safeFileName('a\\b.json'), 'a-b.json');
});

test('a file name has no leading dot, no control character, no reserved name, and a default', () => {
  assert.equal(safeFileName('..'), 'campaign-archive.json');
  assert.equal(safeFileName('.hidden.json'), 'hidden.json');
  assert.equal(safeFileName('a\u0000b\nc.json'), 'a-b-c.json');
  assert.equal(safeFileName('CON.json'), 'campaign-archive.json');
  assert.equal(safeFileName('nul'), 'campaign-archive.json');
  assert.equal(safeFileName('console.json'), 'console.json');
  assert.equal(safeFileName('name.json. '), 'name.json');
  assert.equal(safeFileName(''), 'campaign-archive.json');
});
