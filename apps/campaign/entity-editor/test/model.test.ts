import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  changesBetween,
  colorName,
  commandBody,
  freshEntity,
  initialsOf,
  isDirty,
  modifier,
  newId,
  overlay,
  parseAliases,
  parseEntity,
  parseIllustrations,
  parseTarget,
  reasons,
  sameTarget,
  savePayload,
  signed,
  targetProp,
  tokenHasImage,
} from '../src/model.ts';

const raw = (extra: Record<string, unknown> = {}) => ({
  id: 'morlox',
  name: 'Morlox',
  aliases: ['le gobelin'],
  kind: 'pnj',
  description: 'Un marchand.',
  notes: 'Doit 40 po.',
  player: null,
  prompt: '',
  background: '',
  behavior: '',
  stats: { FOR: 8, DEX: 18, CON: 10, INT: 13, SAG: 9, CHA: 12 },
  modifiers: { FOR: -1, DEX: 4, CON: 0, INT: 1, SAG: -1, CHA: 1 },
  ca: 13,
  pvMax: 18,
  features: [{ name: 'Fuite agile', text: 'Se cache.' }],
  color: 'secondary',
  initials: 'M',
  portrait: false,
  tokenImage: false,
  crop: null,
  ddb: null,
  ...extra,
});

const sheet = (extra: Record<string, unknown> = {}) => {
  const entity = parseEntity(raw(extra));
  assert.ok(entity);
  return entity;
};

test('AbilityModifier@1: the modifier rounds toward negative infinity and is written with its sign', () => {
  assert.equal(modifier(18), 4);
  assert.equal(modifier(10), 0);
  assert.equal(modifier(9), -1);
  assert.equal(modifier(8), -1);
  assert.equal(modifier(1), -5);
  assert.equal(signed(18), '+4');
  assert.equal(signed(10), '+0');
  assert.equal(signed(9), '−1');
});

test('TokenAppearance@1: the initials are the first letters of the first two words, or the first two letters of one word', () => {
  assert.equal(initialsOf('Elowen Ventargent'), 'EV');
  assert.equal(initialsOf('  morlox  '), 'MO');
  assert.equal(initialsOf('Brannoc de la Forge'), 'BD');
  assert.equal(initialsOf(''), '');
  assert.equal(initialsOf('x'), 'X');
});

test('default-token: with neither a portrait nor a token image the token is the initials on the colour', () => {
  assert.equal(tokenHasImage(sheet()), false);
  assert.equal(tokenHasImage(sheet({ portrait: true })), true);
  assert.equal(tokenHasImage(sheet({ tokenImage: true })), true);
  // A choice of the DM weighs more than what the read says.
  assert.equal(tokenHasImage({ ...sheet({ portrait: true }), portraitMedia: null }), false);
  assert.equal(tokenHasImage({ ...sheet(), tokenMedia: 'ill-1' }), true);
  // The colour ends up in a CSS variable: only a name goes through.
  assert.equal(colorName('success'), 'success');
  assert.equal(colorName('red; background: url(x)'), 'accent');
});

test('the sheet of the Capability is read; null, an unknown kind and a missing id are no sheet', () => {
  const morlox = sheet();
  assert.equal(morlox.kind, 'pnj');
  assert.equal(morlox.stats.DEX, 18);
  assert.equal(morlox.crop.zoom, 100);
  assert.equal(morlox.ddb, null);
  for (const data of [null, undefined, 'x', [], raw({ kind: 'dragon' }), raw({ id: '' })]) assert.equal(parseEntity(data), null);
  // A place has no sheet: no token colour either.
  const place = parseEntity({ id: 'p', name: 'Taverne', kind: 'lieu', aliases: [], description: '', notes: '' });
  assert.equal(place?.color, '');
  const linked = sheet({ kind: 'pj', player: 'jou-julie', ddb: { url: 'https://www.dndbeyond.com/characters/1', syncedAt: '2026-09-26T18:12:00' } });
  assert.deepEqual(linked.ddb, { url: 'https://www.dndbeyond.com/characters/1', syncedAt: '2026-09-26T18:12:00' });
  assert.equal(linked.player, 'jou-julie');
});

test('the prop asks for an element, or for a sheet to create', () => {
  assert.deepEqual(parseTarget('morlox'), { id: 'morlox' });
  assert.deepEqual(parseTarget('new:pnj'), { fresh: 'pnj' });
  assert.deepEqual(parseTarget('new:evenement'), { fresh: 'evenement' });
  for (const prop of [undefined, '', 'new:dragon', 3]) assert.equal(parseTarget(prop), null);
  assert.equal(targetProp({ fresh: 'lieu' }), 'new:lieu');
  assert.equal(targetProp({ id: 'a' }), 'a');
  assert.ok(sameTarget({ id: 'a' }, { id: 'a' }));
  assert.ok(!sameTarget({ id: 'a' }, { fresh: 'pj' }));
  assert.ok(!sameTarget(null, { id: 'a' }));
});

test('aliases are typed separated by commas; blanks go', () => {
  assert.deepEqual(parseAliases(' Elowen, la rôdeuse ,, '), ['Elowen', 'la rôdeuse']);
  assert.deepEqual(parseAliases(''), []);
});

test('explicit-save-only: the sheet is modified when the draft is not what the server holds', () => {
  const stored = sheet();
  const draft = structuredClone(stored);
  assert.equal(isDirty(draft, stored), false);
  draft.description = 'Autre.';
  assert.equal(isDirty(draft, stored), true);
  draft.description = stored.description;
  assert.equal(isDirty(draft, stored), false);
  draft.stats.FOR = 9;
  assert.equal(isDirty(draft, stored), true);
  draft.stats.FOR = 8;
  draft.portraitMedia = null;
  assert.equal(isDirty(draft, stored), true);
  draft.portraitMedia = undefined;
  draft.features.push({ name: 'x', text: 'y' });
  assert.equal(isDirty(draft, stored), true);
});

test('what is saved is the fields the DM edits under entity; a modifier is never sent', () => {
  const entity = sheet({ kind: 'pj', player: 'jou-julie' });
  const payload = savePayload({ ...entity, name: ' Elowen ' }, 'camp', false);
  assert.deepEqual(Object.keys(payload), ['entity']);
  const sent = payload.entity as Record<string, unknown>;
  assert.equal(sent.id, 'morlox');
  assert.equal(sent.name, 'Elowen');
  assert.equal(sent.player, 'jou-julie');
  assert.deepEqual(sent.stats, entity.stats);
  assert.ok(!('modifiers' in sent) && !('modifier' in sent));
  assert.ok(!('portrait' in sent) && !('token' in sent));
  // A new element names its campaign; an NPC has no player; a media chosen is sent, a media removed is null.
  const created = savePayload({ ...sheet({ player: 'x' }), portraitMedia: 'ill-1', tokenMedia: null }, 'camp', true);
  assert.equal(created.campaign, 'camp');
  const made = created.entity as Record<string, unknown>;
  assert.equal(made.player, null);
  assert.equal(made.portrait, 'ill-1');
  assert.equal(made.token, null);
  // A place has no sheet.
  const place = parseEntity({ id: 'p', name: 'Taverne', kind: 'lieu', aliases: ['la taverne'], description: 'd', notes: 'n' });
  assert.ok(place);
  assert.deepEqual(savePayload(place, 'camp', false), { entity: { id: 'p', kind: 'lieu', name: 'Taverne', aliases: ['la taverne'], description: 'd', notes: 'n' } });
});

test('default-token: a crop is sent only with a portrait, the one the server holds or the one just picked', () => {
  const cropOf = (e: Parameters<typeof savePayload>[0]) => (savePayload(e, 'camp', false).entity as Record<string, unknown>).crop;
  const base = sheet();
  assert.deepEqual(cropOf({ ...base, portrait: true }), base.crop);
  assert.equal(cropOf({ ...base, portrait: false }), null);
  assert.deepEqual(cropOf({ ...base, portrait: false, portraitMedia: 'ill-1' }), base.crop);
  assert.equal(cropOf({ ...base, portrait: true, portraitMedia: null }), null);
});

test('a command names its DataCapability and its version, its target and its key', () => {
  assert.deepEqual(commandBody('ai.proposeCharacter', { prompt: 'x' }, { aggregate: 'Entity', id: 'a' }, 'k'), {
    dataCapability: 'ai.proposeCharacter@1',
    payload: { prompt: 'x' },
    target: { aggregate: 'Entity', id: 'a' },
    idempotencyKey: 'k',
    caller: { by: 'dm', from: 'EntityEditor', roles: ['role:approver'] },
  });
});

test('every command carries the approver role: the DM approves their own act on the sheet', () => {
  for (const id of ['campaign.saveEntity', 'ai.proposeCharacter', 'dndbeyond.importCharacter', 'dndbeyond.resyncCharacter']) {
    const body = commandBody(id, {}, { aggregate: 'Entity', id: 'a' }, 'k') as { caller: { by: string; roles: string[] } };
    assert.deepEqual(body.caller.roles, ['role:approver'], id);
  }
});

test('the changes between two sheets are worded as the history words them', () => {
  const before = sheet();
  const after = { ...structuredClone(before), pvMax: 21, background: 'Chassé de sa tribu.', features: [...before.features, { name: 'Marchandage', text: '+2' }] };
  after.stats.DEX = 15;
  assert.deepEqual(changesBetween(before, after), [
    { field: 'Background', old: '', new: 'Chassé de sa tribu.' },
    { field: 'PV max', old: '18', new: '21' },
    { field: 'Sorts et capacités', old: 'Fuite agile', new: 'Fuite agile, Marchandage' },
    { field: 'DEX', old: '18', new: '15' },
  ]);
  assert.deepEqual(changesBetween(before, structuredClone(before)), []);
});

test('what the AI changed goes on the draft; what the DM typed elsewhere stays', () => {
  const before = sheet();
  const after = { ...structuredClone(before), background: 'Neuf.', pvMax: 21, ddb: { url: 'u', syncedAt: null }, portrait: true };
  after.stats.DEX = 15;
  const draft = { ...structuredClone(before), notes: 'Tapé par le MJ.', pvMax: 25 };
  const next = overlay(draft, before, after);
  assert.equal(next.background, 'Neuf.');
  assert.equal(next.pvMax, 21);
  assert.equal(next.stats.DEX, 15);
  assert.equal(next.notes, 'Tapé par le MJ.');
  assert.deepEqual(next.ddb, { url: 'u', syncedAt: null });
  assert.equal(next.portrait, true);
  // The draft given is not touched.
  assert.equal(draft.pvMax, 25);
});

test('a sheet to create has nothing saved and a colour to fall back on', () => {
  const pnj = freshEntity('pnj', 'Gob');
  assert.equal(pnj.id, '');
  assert.equal(pnj.color, 'accent');
  assert.equal(pnj.stats.DEX, 10);
  assert.equal(freshEntity('lieu').color, '');
  assert.equal(isDirty(structuredClone(pnj), pnj), false);
});

test('the illustrations of the library are what a portrait or a token can be', () => {
  assert.deepEqual(parseIllustrations({ cover: 'c', battlemaps: [{ id: 'bm', title: 'x' }], illustrations: [{ id: 'ill-1', title: 'Salle' }, { title: 'sans id' }] }), [{ id: 'ill-1', title: 'Salle' }]);
  assert.deepEqual(parseIllustrations(null), []);
});

test('a new element has a uuid of its own', () => {
  const id = newId();
  assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.notEqual(newId(), id);
});

test('a violation is worded; an unknown one is shown as it is', () => {
  assert.match(reasons(['alias-unique-in-campaign']), /alias/);
  assert.equal(reasons(['nothing-known']), 'nothing-known');
  assert.equal(reasons([]), '');
});
