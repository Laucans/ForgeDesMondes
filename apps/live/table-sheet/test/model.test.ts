import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, illustrationFor, modifierText, parseEntity, parseMedia, parseProps, parseRefs, parseTarget, reasons, refOf, settled } from '../src/model.ts';

const MEDIA = parseMedia({
  illustrations: [
    { id: 'ill-tavern', title: 'La salle', art: 'dusk', glyph: 'S', place: 'taverne' },
    { id: 'ill-gaspard', title: 'Gaspard', art: 'gold', glyph: 'G', place: null, file: 'g.png', size: '860 Ko' },
    { title: 'no id' },
  ],
});

test('the props name an element by a known type and an id, and a campaign', () => {
  assert.deepEqual(parseProps({ type: 'place', id: 'taverne', campaign: 'c-1' }), { type: 'place', id: 'taverne', campaign: 'c-1' });
  assert.deepEqual(parseProps({ type: 'monster', id: 4 }), { type: '', id: '', campaign: '' });
  assert.deepEqual(parseTarget({ type: 'illustration', id: 'ill' }), { type: 'illustration', id: 'ill' });
});

test('the kind of an answer gives the type and PC or NPC; the props’ type stands in without a kind', () => {
  const pc = parseEntity({ id: 'e', name: 'Elowen', kind: 'pj' }, { type: '', id: 'e' });
  assert.equal(pc?.type, 'character');
  assert.equal(pc?.pc, true);
  assert.equal(parseEntity({ id: 'm', name: 'Morlox', kind: 'pnj' }, { type: '', id: 'm' })?.pc, false);
  assert.equal(parseEntity({ id: 'l', name: 'Taverne', kind: 'lieu' }, { type: '', id: 'l' })?.type, 'place');
  assert.equal(parseEntity({ id: 'v', name: 'Incendie', kind: 'evenement' }, { type: '', id: 'v' })?.type, 'event');
  assert.equal(parseEntity({ id: 'v', name: 'Incendie' }, { type: 'event', id: 'v' })?.type, 'event');
  assert.equal(parseEntity({ id: 'x', name: 'X', stats: { FOR: 10 } }, { type: '', id: 'x' })?.type, 'character');
  assert.equal(parseEntity({ id: 'x', name: 'X' }, { type: '', id: 'x' })?.type, 'place');
  assert.equal(parseEntity({ id: 'j', name: 'J', player: 'jou-1' }, { type: 'character', id: 'j' })?.pc, true);
});

test('a kind that is a name of Object’s own is no kind', () => {
  assert.equal(parseEntity({ id: 'x', name: 'X', kind: 'constructor' }, { type: 'event', id: 'x' })?.type, 'event');
  assert.equal(parseEntity({ id: 'x', name: 'X', kind: 'toString' }, { type: '', id: 'x' })?.type, 'place');
});

test('no answer, or one with no id, is no element', () => {
  assert.equal(parseEntity(null, { type: '', id: 'x' }), null);
  assert.equal(parseEntity({ name: 'No id' }, { type: '', id: 'x' }), null);
});

test('the sheet of a character keeps the six scores, the vitals and the features, nothing else', () => {
  const e = parseEntity(
    { id: 'm', name: 'Morlox', kind: 'pnj', stats: { FOR: 8, DEX: 14, CON: 10, INT: 13, SAG: 9, CHA: 12 }, modifiers: { DEX: 2 }, ca: 13, pvMax: 18, speed: '9 m', features: [{ name: 'Dague', text: 'Perforant.' }, { x: 1 }], token: 'secret' },
    { type: '', id: 'm' },
  );
  assert.deepEqual(e?.stats, { FOR: 8, DEX: 14, CON: 10, INT: 13, SAG: 9, CHA: 12 });
  assert.deepEqual(e?.modifiers, { FOR: null, DEX: 2, CON: null, INT: null, SAG: null, CHA: null });
  assert.deepEqual([e?.ca, e?.pvMax, e?.speed], [13, 18, '9 m']);
  assert.deepEqual(e?.features, [{ name: 'Dague', text: 'Perforant.' }]);
  assert.equal(JSON.stringify(e).includes('secret'), false);
});

test('a number of metres is a speed all the same, and an initials-less answer takes the name’s first letter', () => {
  assert.equal(parseEntity({ id: 'a', name: 'Ankou', kind: 'pnj', speed: 9 }, { type: '', id: 'a' })?.speed, '9');
  assert.equal(parseEntity({ id: 'a', name: 'Ankou', kind: 'pnj' }, { type: '', id: 'a' })?.initials, 'A');
});

test('the modifier is the answer’s, else computed from the score, and written with a true minus', () => {
  assert.equal(modifierText(18, null), '+4');
  assert.equal(modifierText(8, null), '−1');
  assert.equal(modifierText(10, null), '+0');
  assert.equal(modifierText(9, null), '−1');
  assert.equal(modifierText(11, null), '+0');
  assert.equal(modifierText(18, 2), '+2');
  assert.equal(modifierText(null, -3), '−3');
  assert.equal(modifierText(null, null), '');
});

test('the characters of a place are ids, with or without a name, each once', () => {
  assert.deepEqual(parseRefs(['gaspard', { id: 'morlox', name: 'Morlox' }, 'gaspard', '', 7]), ['gaspard', 'morlox']);
  assert.deepEqual(parseRefs(undefined), []);
});

test('a character met is named by its own sheet, and one with no sheet is dropped', () => {
  assert.deepEqual(refOf({ id: 'g', name: 'Gaspard', kind: 'pnj' }), { id: 'g', name: 'Gaspard', pc: false });
  assert.deepEqual(refOf({ id: 'e', name: 'Elowen', kind: 'pj' }), { id: 'e', name: 'Elowen', pc: true });
  assert.equal(refOf(null), null);
  assert.equal(refOf({ id: 'g', name: '' }), null);
});

test('an illustration is the one the answer links, else, for a place, the one the library attaches to it', () => {
  const place = parseEntity({ id: 'taverne', name: 'Taverne', kind: 'lieu' }, { type: '', id: 'taverne' });
  assert.ok(place);
  assert.equal(illustrationFor(place, MEDIA)?.id, 'ill-tavern');
  const linked = parseEntity({ id: 'g', name: 'Gaspard', kind: 'pnj', illustration: 'ill-gaspard' }, { type: '', id: 'g' });
  assert.ok(linked);
  assert.equal(illustrationFor(linked, MEDIA)?.id, 'ill-gaspard');
  // A link to an image the library does not list is no illustration; a character is not found by place.
  const lost = parseEntity({ id: 'taverne', name: 'X', kind: 'pnj', illustration: 'ill-gone' }, { type: '', id: 'taverne' });
  assert.ok(lost);
  assert.equal(illustrationFor(lost, MEDIA), undefined);
  const bare = parseEntity({ id: 'v', name: 'Incendie', kind: 'evenement' }, { type: '', id: 'v' });
  assert.ok(bare);
  assert.equal(illustrationFor(bare, MEDIA), undefined);
});

test('the library is reduced to its illustrations', () => {
  assert.deepEqual(MEDIA.illustrations.map((i) => i.id), ['ill-tavern', 'ill-gaspard']);
  assert.deepEqual(MEDIA.illustrations[1], { id: 'ill-gaspard', title: 'Gaspard', art: 'gold', glyph: 'G', place: '', file: 'g.png', size: '860 Ko' });
  assert.deepEqual(parseMedia(undefined), { illustrations: [] });
});

test('a send is a command by identifier, aimed at the diffusion, from the sheet', () => {
  assert.deepEqual(commandBody('live.showOnPlayerView', { kind: 'illustration', media: 'ill' }), {
    dataCapability: 'live.showOnPlayerView@1',
    target: { aggregate: 'Diffusion' },
    payload: { kind: 'illustration', media: 'ill' },
    caller: { by: 'dm', from: 'TableSheet', roles: [] },
  });
});

test('a refusal is told in words, and an unknown id as it is', () => {
  assert.equal(reasons(['window-proportional', 'other']), 'la fenêtre doit garder les proportions de l’image, other');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['base-coherent', 3] } }), { status: 'rejected', violations: ['base-coherent'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
});
