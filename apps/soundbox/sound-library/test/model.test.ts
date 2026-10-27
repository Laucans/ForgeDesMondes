import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  addPayload,
  addProblem,
  campaignOf,
  commandBody,
  duration,
  editProblem,
  filterSounds,
  isSoundFile,
  nameFromFile,
  parseSounds,
  reasons,
  settled,
  splitTags,
  summary,
  tagsOf,
  updatePayload,
} from '../src/model.ts';
import type { Sound } from '../src/types.ts';

const sound = (id: string, name: string, tags: string[]): Sound => ({ id, name, tags, volume: 70, file: `${id}.ogg`, duration: 2, playing: false });
const library = [
  sound('porte', 'Porte qui grince', ['porte', 'bois', 'intérieur']),
  sound('epees', 'Épées qui s’entrechoquent', ['combat', 'métal']),
  sound('chope', 'Chope qui se brise', ['taverne', 'verre']),
  sound('foule', 'Bruit de foule en taverne', ['foule', 'taverne', 'intérieur']),
];

test('an empty or malformed answer is an empty list, not an error', () => {
  assert.deepEqual(parseSounds(undefined), []);
  assert.deepEqual(parseSounds({ sounds: 'x' }), []);
  assert.deepEqual(parseSounds({ sounds: [null, { name: 'sans id' }, 3] }), []);
});

test('a sound is read with what the Capability gives, and safe defaults for the rest', () => {
  const [one] = parseSounds({ sounds: [{ id: 'a', name: 'A', tags: ['x', 4, 'y'], volume: 55, file: 'a.ogg', duration: 1.5, gain: 0.44, playing: true }, { id: 'b' }] });
  assert.deepEqual(one, { id: 'a', name: 'A', tags: ['x', 'y'], volume: 55, file: 'a.ogg', duration: 1.5, playing: true });
  assert.deepEqual(parseSounds({ sounds: [{ id: 'b' }] }), [{ id: 'b', name: '', tags: [], volume: 0, file: '', duration: null, playing: false }]);
});

test('the search reads the name and the tags, without case or accents', () => {
  assert.deepEqual(filterSounds(library, 'porte', null).map((s) => s.id), ['porte']);
  assert.deepEqual(filterSounds(library, 'EPEES', null).map((s) => s.id), ['epees']);
  assert.deepEqual(filterSounds(library, 'taverne', null).map((s) => s.id), ['chope', 'foule']);
  assert.deepEqual(filterSounds(library, '  ', null).length, 4);
  assert.deepEqual(filterSounds(library, 'inconnu', null), []);
});

test('the search does not read the file name or the volume', () => {
  assert.deepEqual(filterSounds(library, 'ogg', null), []);
  assert.deepEqual(filterSounds(library, '70', null), []);
});

test('a tag keeps the sounds that carry it, and combines with the search', () => {
  assert.deepEqual(filterSounds(library, '', 'intérieur').map((s) => s.id), ['porte', 'foule']);
  assert.deepEqual(filterSounds(library, '', 'Interieur').map((s) => s.id), ['porte', 'foule']);
  assert.deepEqual(filterSounds(library, 'foule', 'intérieur').map((s) => s.id), ['foule']);
  assert.deepEqual(filterSounds(library, '', 'inter'), [], 'a tag is chosen whole, not by a part of it');
});

test('the tags of the library are listed once each', () => {
  assert.deepEqual(tagsOf(library), ['porte', 'bois', 'intérieur', 'combat', 'métal', 'taverne', 'verre', 'foule']);
  assert.equal(tagsOf([sound('a', 'A', ['Taverne']), sound('b', 'B', ['taverne'])]).length, 1);
});

test('the summary counts the sounds and the tags', () => {
  assert.equal(summary(library), '4 sons · 8 tags');
  assert.equal(summary([sound('a', 'A', ['x'])]), '1 son · 1 tag');
  assert.equal(summary([]), '0 son · 0 tag');
});

test('a Tags field becomes a list: commas, spaces and hash signs go', () => {
  assert.deepEqual(splitTags('porte, bois , #intérieur,, '), ['porte', 'bois', 'intérieur']);
  assert.deepEqual(splitTags(''), []);
});

test('a dropped file suggests a name', () => {
  assert.equal(nameFromFile('volet-qui_claque.ogg'), 'Volet qui claque');
  assert.equal(nameFromFile('.ogg'), '');
});

test('only the sound files are taken', () => {
  assert.ok(isSoundFile('porte.OGG') && isSoundFile('a.mp3') && isSoundFile('b.wav'));
  assert.ok(!isSoundFile('notes.pdf') && !isSoundFile('ogg'));
});

test('a duration is written in French', () => {
  assert.equal(duration(2.4), '2,4 s');
  assert.equal(duration(null), '—');
});

test('the add form needs a file and a name', () => {
  assert.match(addProblem({ file: '', name: 'x', tags: '' }), /fichier/);
  assert.match(addProblem({ file: 'a.ogg', name: '  ', tags: '' }), /nom/);
  assert.equal(addProblem({ file: 'a.ogg', name: 'x', tags: '' }), '');
});

test('the edit form needs a name and a volume in range', () => {
  assert.match(editProblem({ id: 'a', name: ' ', tags: '', volume: 50 }), /nom/);
  assert.match(editProblem({ id: 'a', name: 'x', tags: '', volume: 120 }), /volume/);
  assert.match(editProblem({ id: 'a', name: 'x', tags: '', volume: Number.NaN }), /volume/);
  assert.equal(editProblem({ id: 'a', name: 'x', tags: '', volume: 0 }), '');
});

test('the payloads are those of the DataCapabilities: volume in percent, tags as a list', () => {
  assert.deepEqual(addPayload({ file: 'a.ogg', name: ' Porte ', tags: 'Bois, porte' }, ''), { name: 'Porte', tags: ['Bois', 'porte'], volume: 75, file: 'a.ogg' });
  assert.deepEqual(addPayload({ file: 'a.ogg', name: 'P', tags: '' }, 'cendres'), { campaign: 'cendres', name: 'P', tags: [], volume: 75, file: 'a.ogg' });
  assert.deepEqual(updatePayload({ id: 'a', name: ' P ', tags: 'x', volume: 40 }), { name: 'P', tags: ['x'], volume: 40 });
});

test('a command names its DataCapability by identifier and version, and its target when it has one', () => {
  const body = commandBody('soundbox.removeSound', {}, 'snd-1') as Record<string, unknown>;
  assert.equal(body.dataCapability, 'soundbox.removeSound@1');
  assert.equal(body.targetId, 'snd-1');
  assert.ok(!('targetId' in (commandBody('soundbox.addSound', {}) as object)));
});

test('a rejected command is told in words, an unknown violation as it is', () => {
  assert.equal(reasons(['sound-volume-range', 'autre']), 'le volume reste entre 0 et 100 %, autre');
});

test('the result of a command is read from the server’s answer, however little it holds', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['x'] } }), { status: 'rejected', violations: ['x'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
});

test('the campaign of the running session is read from its header, under `session`', () => {
  assert.equal(campaignOf({ session: { id: 's4', campaign: 'cendres-valombre' }, microphone: { on: true } }), 'cendres-valombre');
  assert.equal(campaignOf({ session: { id: 's4' }, campaign: 'ignored' }), '', 'a campaign beside the session is not the session’s');
});

test('no session, or an answer with no campaign, is no campaign', () => {
  assert.equal(campaignOf(null), '');
  assert.equal(campaignOf(undefined), '');
  assert.equal(campaignOf({ session: { campaign: 3 } }), '');
  assert.equal(campaignOf([]), '');
});
