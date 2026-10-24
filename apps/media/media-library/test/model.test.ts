import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { association, addPlaceCommand, parseMedia, parsePlaces, parseProps, parseSessionPlaces, reasons, setPlaceCommand, stem, titleFor, uploadCommand } from '../src/model.ts';

const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));
const media = parseMedia(fixture('media.listMedia'));
const byId = (id: string) => {
  const found = media.find((m) => m.id === id);
  assert.ok(found, `${id} is in the fixture`);
  return found;
};

test('the library is the battlemaps then the illustrations, each with its place or none', () => {
  assert.deepEqual(media.map((m) => m.kind), ['battlemap', 'battlemap', 'illustration', 'illustration', 'illustration', 'illustration', 'illustration']);
  assert.equal(byId('bm-gue').place, null);
  assert.equal(byId('bm-moulin').place, 'moulin');
  assert.equal(byId('bm-moulin').grid?.orient, 'flat');
  assert.equal(byId('ill-cover').grid, null);
});

test('what the Capabilities answer too little of is an empty library, not an error', () => {
  assert.deepEqual(parseMedia(undefined), []);
  assert.deepEqual(parseMedia({ battlemaps: 'x', illustrations: [null, { title: 'no id' }] }), []);
  assert.deepEqual(parsePlaces({ not: 'a list' }), []);
  assert.equal(parseSessionPlaces(null).size, 0);
});

test('session-media-by-place: a medium is in the session when its place is in a scene; no place is attached to nothing', () => {
  const places = parseSessionPlaces(fixture('session.listScenes'));
  assert.deepEqual([...places].sort(), ['taverne-sanglier', 'valombre']);
  assert.deepEqual(association(byId('ill-cover'), places), { kind: 'in', place: 'valombre' });
  assert.deepEqual(association(byId('bm-moulin'), places), { kind: 'out', place: 'moulin' });
  assert.deepEqual(association(byId('ill-gaspard'), places), { kind: 'none' });
  // No session known: nothing is attached, whatever the place.
  assert.deepEqual(association(byId('ill-cover'), null), { kind: 'none' });
});

test('a place listed in the campaign has an id and a name', () => {
  const places = parsePlaces(fixture('campaign.listPlaces'));
  assert.deepEqual(places.map((p) => p.id), ['taverne-sanglier', 'valombre', 'moulin', 'tour-ebene']);
  assert.equal(places[0]?.name, 'La Taverne du Sanglier');
});

test('the session number is a positive integer, else the card does not number the session', () => {
  assert.deepEqual(parseProps({ campaign: 'cendres-valombre', session: 's4', sessionNumber: 4 }), { campaign: 'cendres-valombre', session: 's4', sessionNumber: 4 });
  assert.deepEqual(parseProps({ session: 's4', sessionNumber: '4' }), { campaign: '', session: 's4', sessionNumber: 4 });
  assert.deepEqual(parseProps({ sessionNumber: 0 }), { campaign: '', session: '', sessionNumber: null });
  assert.deepEqual(parseProps({ session: 3 }), { campaign: '', session: '', sessionNumber: null });
});

test('each command names its DataCapability by identifier and the aggregate it targets', () => {
  assert.deepEqual(setPlaceCommand('bm-gue', null, 'k1'), {
    dataCapability: 'media.setMediaPlace@1',
    payload: { media: 'bm-gue', place: null },
    target: { aggregate: 'Media', id: 'bm-gue' },
    idempotencyKey: 'k1',
    caller: { by: 'dm', from: 'MediaLibrary', roles: [] },
  });
  const add = addPlaceCommand('s4', 'moulin', 'k2') as { dataCapability: string; payload: unknown; target: unknown };
  assert.equal(add.dataCapability, 'session.addPlaceToSession@1');
  assert.deepEqual(add.payload, { session: 's4', place: 'moulin' });
  assert.deepEqual(add.target, { aggregate: 'Session', id: 's4' });
});

test('an upload with no place leaves the place out, and the size when it is unknown', () => {
  const bare = uploadCommand({ campaign: 'c1', kind: 'illustration', title: 'T', file: 't.png', place: null, size: null }, 'k') as { payload: Record<string, unknown>; target?: unknown };
  assert.deepEqual(bare.payload, { campaign: 'c1', kind: 'illustration', title: 'T', file: 't.png' });
  assert.equal(bare.target, undefined);
  const full = uploadCommand({ campaign: 'c1', kind: 'battlemap', title: 'T', file: 't.png', place: 'moulin', size: { width: 900, height: 600 } }, 'k') as { payload: Record<string, unknown> };
  assert.deepEqual(full.payload, { campaign: 'c1', kind: 'battlemap', title: 'T', file: 't.png', width: 900, height: 600, place: 'moulin' });
});

test('the title falls back to the file, then to « Sans titre »', () => {
  assert.equal(stem('la-cave_du-sanglier.jpg'), 'la cave du sanglier');
  assert.equal(titleFor({ kind: 'battlemap', title: '  Ma carte ', place: '' }, { name: 'x.jpg' }), 'Ma carte');
  assert.equal(titleFor({ kind: 'battlemap', title: '', place: '' }, { name: 'la-cave.jpg' }), 'la cave');
  assert.equal(titleFor({ kind: 'battlemap', title: '', place: '' }, null), 'Sans titre');
});

test('a refusal is told in words, an unknown id as it is', () => {
  assert.equal(reasons(['media-place-is-lieu']), 'ce lieu n’est pas un lieu de la campagne');
  assert.equal(reasons(['media-place-is-lieu', 'other-rule']), 'ce lieu n’est pas un lieu de la campagne, other-rule');
});
