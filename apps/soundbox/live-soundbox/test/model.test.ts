import assert from 'node:assert/strict';
import { test } from 'node:test';
import { allTags, commandBody, commandId, fold, parseLibrary, reasons, settled, tagKey, visible } from '../src/model.ts';

const sound = (id: string, name: string, tags: string[]) => ({ id, name, tags, duration: 2, volume: 50, file: `${id}.ogg`, gain: 0.4, playing: false });

test('an empty answer is an empty library, not an error', () => {
  assert.deepEqual(parseLibrary(undefined), { masterVolume: 0, sounds: [] });
  assert.deepEqual(parseLibrary({ sounds: 'nope' }), { masterVolume: 0, sounds: [] });
});

test('a sound plays at the gain the Capability gives, silent when it gives none', () => {
  const library = parseLibrary({ masterVolume: 80, sounds: [sound('a', 'A', []), { id: 'b', name: 'B', tags: [], volume: 55, file: 'b.ogg' }, { name: 'no id' }] });
  assert.equal(library.sounds.length, 2);
  assert.equal(library.sounds[0]?.gain, 0.4);
  assert.equal(library.sounds[1]?.gain, 0, 'the gain is never worked out from the volumes');
  assert.equal(library.sounds[1]?.playing, false);
});

test('volumes and gains stay in their range, a duration is kept only when it is a length', () => {
  const [loud, odd] = parseLibrary({ masterVolume: 150, sounds: [{ ...sound('a', 'A', []), volume: 400, gain: 3 }, { ...sound('b', 'B', []), duration: 0, volume: -5, gain: -1 }] }).sounds;
  assert.equal(parseLibrary({ masterVolume: 150 }).masterVolume, 100);
  assert.deepEqual([loud?.volume, loud?.gain, loud?.duration], [100, 1, 2]);
  assert.deepEqual([odd?.volume, odd?.gain, odd?.duration], [0, 0, null]);
});

test('a search reads the name and the tags, whatever the case or the accents', () => {
  const sounds = [sound('1', 'Épées qui s’entrechoquent', ['combat']), sound('2', 'Porte qui grince', ['bois', 'intérieur'])];
  const ids = (query: string, tags: string[] = []) => visible(sounds, query, new Set(tags)).map((s) => s.id);
  assert.deepEqual(ids('EPEES'), ['1']);
  assert.deepEqual(ids('interieur'), ['2']);
  assert.deepEqual(ids('  '), ['1', '2']);
  assert.deepEqual(ids('zzz'), []);
  assert.deepEqual(ids('qui'), ['1', '2'], 'a word of the name');
  assert.deepEqual(ids('qui bois'), [], 'a query does not run across the name and a tag');
  assert.deepEqual(visible([sound('3', 'Porte', ['bois'])], 'porte bois', new Set()), [], 'nor across the name and the first tag');
});

test('a tag filter keeps the sounds that carry at least one chosen tag, the accents kept', () => {
  const sounds = [sound('1', 'A', ['Intérieur ', 'bois']), sound('2', 'B', ['interieur']), sound('3', 'C', ['combat'])];
  const ids = (tags: string[]) => visible(sounds, '', new Set(tags)).map((s) => s.id);
  assert.deepEqual(ids(['intérieur']), ['1']);
  assert.deepEqual(ids(['combat', 'bois']), ['1', '3']);
  assert.deepEqual(ids([]), ['1', '2', '3']);
  assert.deepEqual(visible(sounds, 'bois', new Set(['combat'])), [], 'search and filter both apply');
  assert.equal(tagKey('  Taverne '), 'taverne');
  assert.equal(fold('Épée'), 'epee');
});

test('the tags of the library are listed once, sorted', () => {
  assert.deepEqual(allTags([sound('1', 'A', ['taverne', 'Bois']), sound('2', 'B', ['bois', ' ', 'arc'])]), ['arc', 'bois', 'taverne']);
});

test('a command names the DataCapability and its version, the target only when it has one', () => {
  assert.deepEqual(commandBody('soundbox.stopAll', {}), { dataCapability: 'soundbox.stopAll@1', payload: {}, caller: { by: 'dm', from: 'LiveSoundbox', roles: [] } });
  assert.deepEqual(commandBody('soundbox.playSound', { a: 1 }, 'snd'), {
    dataCapability: 'soundbox.playSound@1',
    payload: { a: 1 },
    target: { id: 'snd' },
    caller: { by: 'dm', from: 'LiveSoundbox', roles: [] },
  });
});

test('a command answer is read as the server writes it', () => {
  assert.deepEqual(settled({ commandId: 'c1', result: { status: 'rejected', violations: ['dm-side-only', 7] } }), { status: 'rejected', violations: ['dm-side-only'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(commandId({ commandId: 'c1' }), 'c1');
  assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
  assert.equal(commandId(undefined), '');
  assert.equal(reasons(['dm-side-only', 'other']), 'seule la DM view joue un son, other');
});
