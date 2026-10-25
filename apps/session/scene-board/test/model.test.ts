import assert from 'node:assert/strict';
import { test } from 'node:test';
import { absentPcs, commandBody, moved, parseCharacters, parseInstance, parseScenes, parseSession, presentPcs, ranks, reasons, sessionAsked, settled } from '../src/model.ts';
import type { Scene } from '../src/types.ts';

const scene = (id: string): Scene => ({ id, title: id, places: [], characters: [], events: [], media: [], toCreate: [], battlemap: null, notes: '' });

test('an unknown answer is an empty session, not an error', () => {
  assert.deepEqual(parseSession(undefined), { id: '', title: '', order: null, campaign: null, present: [] });
});

test('the scenes are in the order of their rank, whatever order the answer is in', () => {
  const scenes = parseScenes([{ id: 'b', order: 2 }, { id: 'a', order: 1 }, { id: 'c', order: 3, battlemap: 'bmi-c' }, { title: 'no id' }]);
  assert.deepEqual(scenes.map((s) => s.id), ['a', 'b', 'c']);
  assert.equal(scenes[2]?.battlemap, 'bmi-c');
  assert.equal(scenes[0]?.battlemap, null);
});

test('present-only-pj: only PCs are offered, and the present ones are those the session lists', () => {
  const characters = parseCharacters([{ id: 'elowen', name: 'Elowen', kind: 'pj' }, { id: 'kael', name: 'Kael', kind: 'pj' }, { id: 'bandit', name: 'Bandit', kind: 'pnj' }]);
  const board = { characters, session: parseSession({ id: 's', present: ['kael', 'bandit', 'ghost'] }) };
  assert.deepEqual(presentPcs(board).map((c) => c.id), ['kael']);
  assert.deepEqual(absentPcs(board).map((c) => c.id), ['elowen']);
});

test('moving a scene swaps it with its neighbour, and no scene leaves the list', () => {
  const scenes = ['a', 'b', 'c'].map(scene);
  assert.deepEqual(moved(scenes, 'c', -1), ['a', 'c', 'b']);
  assert.deepEqual(moved(scenes, 'a', 1), ['b', 'a', 'c']);
  assert.equal(moved(scenes, 'a', -1), null);
  assert.equal(moved(scenes, 'c', 1), null);
  assert.equal(moved(scenes, 'zz', 1), null);
});

test('scene-order-contiguous: a reorder gives every scene its rank, from 1 to n', () => {
  assert.deepEqual(ranks(['c', 'a', 'b']), [{ scene: 'c', rank: 1 }, { scene: 'a', rank: 2 }, { scene: 'b', rank: 3 }]);
});

test('an instance counts its tokens and the hidden ones', () => {
  assert.deepEqual(parseInstance({ id: 'i', battlemap: 'm', state: { tokens: [{ visible: true }, { visible: false }, {}] } }), { id: 'i', battlemap: 'm', tokens: 3, hidden: 1 });
  assert.equal(parseInstance({ battlemap: 'm' }), null);
});

test('a command names its DataCapability and its version, and keeps its idempotency key only when given', () => {
  const body = commandBody('session.addScene', { session: 's' }, { aggregate: 'Session', id: 's' }) as Record<string, unknown>;
  assert.equal(body.dataCapability, 'session.addScene@1');
  assert.equal('idempotencyKey' in body, false);
  assert.equal((commandBody('x.y', {}, { aggregate: 'A', id: 'i' }, 'k') as Record<string, unknown>).idempotencyKey, 'k');
});

test('a rejection is told in words, an unknown violation as it is', () => {
  assert.equal(reasons(['present-only-pj', 'other']), 'seul un PJ de la campagne peut être présent, other');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['v'] } }), { status: 'rejected', violations: ['v'] });
});

test('the session asked is the prop, else the context value, else none', () => {
  assert.equal(sessionAsked('s4', 's3'), 's4');
  assert.equal(sessionAsked(undefined, 's3'), 's3');
  assert.equal(sessionAsked('', 's3'), 's3');
  assert.equal(sessionAsked(42, undefined), '');
  assert.equal(sessionAsked(undefined, undefined), '');
});
