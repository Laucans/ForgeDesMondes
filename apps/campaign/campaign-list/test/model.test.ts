import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import {
  createCommand,
  dayText,
  exportCommand,
  failed,
  glyphOf,
  importCommand,
  lastPlayedText,
  parseArchive,
  parseCampaigns,
  refusedBy,
  safeFileName,
  sessionsText,
  settled,
  unapplied,
  type Words,
} from '../src/model.ts';

const fixture: unknown = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/campaign.listCampaigns.json', import.meta.url), 'utf8'));

test('the Capability’s fixture is read as it is: one card per campaign, every field kept', () => {
  const campaigns = parseCampaigns(fixture);
  assert.equal(campaigns.length, 3);
  assert.deepEqual(campaigns[1], {
    id: 'nuit-carnaval',
    name: 'La Nuit du Carnaval Pourpre',
    type: 'oneshot',
    art: 'dusk',
    glyph: 'C',
    pitch: 'Un masque volé, une ville en fête, six heures avant l’aube.',
    sessions: 1,
    lastPlayed: '2026-07-11',
    next: null,
  });
});

test('an answer that is no list, an item with no id or no name, is not drawn; an unknown count stays unknown', () => {
  assert.deepEqual(parseCampaigns(null), []);
  assert.deepEqual(parseCampaigns({ nope: 1 }), []);
  const kept = parseCampaigns([{ id: '', name: 'x' }, { id: 'a' }, 7, { id: 'b', name: 'B', type: 'whatever', sessions: null, art: null, glyph: null, pitch: null, lastPlayed: '', next: '' }]);
  assert.equal(kept.length, 1);
  assert.deepEqual(kept[0], { id: 'b', name: 'B', type: 'campagne', art: '', glyph: '', pitch: '', sessions: null, lastPlayed: null, next: null });
  assert.equal(parseCampaigns({ campaigns: [{ id: 'c', name: 'C' }] }).length, 1);
});

test('the words: plural sessions, none while the count is unknown, never played, a day in French', () => {
  assert.equal(sessionsText(0), '0 séance');
  assert.equal(sessionsText(1), '1 séance');
  assert.equal(sessionsText(4), '4 séances');
  assert.equal(sessionsText(null), '');
  assert.equal(lastPlayedText(null), 'jamais jouée');
  assert.equal(lastPlayedText('2026-10-03'), `jouée le ${dayText('2026-10-03')}`);
  assert.match(dayText('2026-10-03'), /3 octobre 2026/);
  assert.equal(dayText('pas une date'), 'pas une date');
  assert.equal(glyphOf('  élan'), 'É');
  assert.equal(glyphOf('   '), '');
});

test('the create command names its DataCapability and version, trims the name, and a blank pitch is none', () => {
  const body = createCommand('campaign.createCampaign', { name: '  Les Brumes ', type: 'oneshot', pitch: '  ' }) as Record<string, unknown>;
  assert.equal(body.dataCapability, 'campaign.createCampaign@1');
  assert.deepEqual(body.payload, { name: 'Les Brumes', type: 'oneshot', pitch: null, glyph: 'L' });
  assert.deepEqual((createCommand('campaign.createCampaign', { name: 'A', type: 'campagne', pitch: ' un pitch ' }) as { payload: unknown }).payload, { name: 'A', type: 'campagne', pitch: 'un pitch', glyph: 'A' });
});

test('the export names the campaign by its id; the import sends the archive and names no campaign', () => {
  assert.deepEqual(exportCommand('campaign.exportCampaign', 'c1'), { dataCapability: 'campaign.exportCampaign@1', payload: { id: 'c1' }, caller: { by: 'dm', from: 'CampaignList', roles: [] } });
  const archive = { format: 'campaign-archive', version: 1 };
  const body = importCommand('campaign.importCampaign', archive) as Record<string, unknown>;
  assert.equal(body.dataCapability, 'campaign.importCampaign@1');
  assert.deepEqual(body.payload, { archive });
  assert.equal(body.targetId, undefined, 'an import never targets the original');
  assert.equal(body.target, undefined);
});

test('a dropped file is an archive when it is a JSON document, and nothing else is', () => {
  assert.deepEqual(parseArchive('{"format":"campaign-archive"}'), { format: 'campaign-archive' });
  assert.equal(parseArchive('not json'), undefined);
  assert.equal(parseArchive('[1,2]'), undefined);
  assert.equal(parseArchive('"text"'), undefined);
});

test('a command’s answer: status, reasons and, once an export is applied, its archive', () => {
  const archive = { file: 'les-cendres-2026.json', content: { campaign: { id: 'c1' } } };
  assert.deepEqual(settled({ commandId: 'c1', result: { status: 'applied', violations: [], attachment: archive } }), { status: 'applied', violations: [], archive });
  assert.equal(settled({ result: { status: 'queued', attachment: null } }).archive, null);
  assert.equal(settled({ result: { status: 'applied', attachment: { file: '', content: {} } } }).archive, null, 'a file with no name is no archive');
  assert.equal(settled({ result: { status: 'applied', attachment: { file: 'x.json' } } }).archive, null, 'nor one with no content');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['bad', 3] } }).violations, ['bad']);
  assert.deepEqual(settled('nope'), { status: '', violations: [], archive: null });
});

test('why a command was not applied: the refusal’s reasons, the wait that ran out, a server that could not be asked', () => {
  const words: Words = { refused: 'Import refusé', pending: 'Import non appliqué', failed: 'L’import a échoué.' };
  assert.equal(unapplied({ status: 'rejected', violations: ['format-unknown', 'version-unknown'], archive: null }, words), 'Import refusé : format-unknown, version-unknown.');
  assert.equal(unapplied({ status: 'rejected', violations: [], archive: null }, words), 'Import refusé : raison inconnue.');
  assert.equal(unapplied({ status: 'queued', violations: [], archive: null }, words), 'Import non appliqué : état queued.');
  assert.equal(unapplied({ status: '', violations: [], archive: null }, words), 'Import non appliqué : état inconnu.');
  assert.equal(unapplied(failed(''), words), 'L’import a échoué.');
  assert.equal(unapplied(failed('archive illisible'), words), 'Import refusé : archive illisible.');
});

test('a refusal before queueing carries the server’s own words; any other failure carries none', () => {
  assert.equal(refusedBy({ status: 422, body: { error: 'format inconnu' } }), 'format inconnu');
  assert.equal(refusedBy({ status: 502, body: 'bad gateway' }), '');
  assert.equal(refusedBy(new Error('offline')), '');
  assert.equal(refusedBy(null), '');
});

test('a download is saved under the name the export gave, with nothing that is a path', () => {
  assert.equal(safeFileName('les-cendres-2026-10-14T10-00-00Z.json'), 'les-cendres-2026-10-14T10-00-00Z.json');
  assert.equal(safeFileName('../../etc/passwd'), '..-..-etc-passwd');
  assert.equal(safeFileName('a:b*c.json'), 'a-b-c.json');
});
