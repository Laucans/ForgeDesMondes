import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, commandId, countLabel, inSession, isFinal, parseItems, parseProposal, reasons, refinementOf, settled, signed } from '../src/model.ts';

const entry = { id: 'tc-a', type: 'pnj', name: 'A', session: 's4', sessionTitle: 'Séance 4', excerpt: 'x', status: 'todo', entity: null };

test('an entry is created if and only if its element is set (to-create-leaves-once-created)', () => {
  const items = parseItems({ items: [
    { ...entry, id: 'a', status: 'created', entity: 'e1' },
    { ...entry, id: 'b', status: 'created', entity: null },
    { ...entry, id: 'c', status: 'todo', entity: 'e2' },
  ] });
  assert.deepEqual(items.map((i) => [i.status, i.entity]), [['created', 'e1'], ['todo', null], ['todo', null]]);
});

test('the answer is read as { items } or as the bare list, and junk reads as empty', () => {
  assert.equal(parseItems([entry]).length, 1);
  assert.equal(parseItems({ items: [entry] }).length, 1);
  assert.deepEqual(parseItems(undefined), []);
  assert.deepEqual(parseItems({ items: [{ name: 'no id' }, 3] }), []);
});

test('the count says how many remain and how many are created', () => {
  const items = parseItems([entry, { ...entry, id: 'b' }, { ...entry, id: 'c', status: 'created', entity: 'e' }]);
  assert.equal(countLabel(items), '2 à créer · 1 créée(s)');
});

test('a session keeps its own entries, and no session keeps all of them', () => {
  const items = parseItems([entry, { ...entry, id: 'b', session: 's5' }]);
  assert.deepEqual(inSession(items, 's5').map((i) => i.id), ['b']);
  assert.equal(inSession(items, '').length, 2);
});

test('the refinement is sent only when the DM wrote one', () => {
  const [item] = parseItems([entry]);
  assert.ok(item);
  assert.equal(refinementOf('  \n '), null);
  assert.equal(refinementOf('  Il boite.  '), 'Il boite.');
  assert.deepEqual(commandBody('ai.createFromBrief', item, null), {
    dataCapability: 'ai.createFromBrief@1',
    payload: { entry: 'tc-a' },
    target: { aggregate: 'Session', id: 's4' },
    caller: { by: 'dm', from: 'ToCreateList', roles: [] },
  });
  assert.deepEqual((commandBody('ai.createFromBrief', item, 'Il boite.') as { payload: unknown }).payload, { entry: 'tc-a', refinement: 'Il boite.' });
});

test('an entry with no session targets no aggregate', () => {
  const [item] = parseItems([{ ...entry, session: null }]);
  assert.ok(item);
  assert.equal('target' in (commandBody('ai.createFromBrief', item, null) as object), false);
});

test('the proposal is the element: its text, and its sheet for an NPC only', () => {
  const npc = parseProposal({ id: 'e', name: 'Odric', description: 'd', aliases: ['le frère', 3], background: 'b', behavior: 'c', stats: { FOR: 10, DEX: 8 }, modifiers: { FOR: 0, DEX: -1 }, ca: 10, pvMax: 13, features: [{ name: 'Bâton', text: 't' }] });
  assert.equal(npc?.background, 'b');
  assert.deepEqual(npc?.aliases, ['le frère']);
  assert.deepEqual(npc?.sheet?.abilities.slice(0, 2), [{ key: 'FOR', score: 10, modifier: 0 }, { key: 'DEX', score: 8, modifier: -1 }]);
  assert.equal(npc?.sheet?.ca, 10);
  assert.equal(parseProposal({ id: 'p', name: 'Gué', description: 'd' })?.sheet, null);
  assert.equal(parseProposal(null), null);
  assert.equal(parseProposal({ name: 'no id' }), null);
});

test('a modifier is written with its sign', () => {
  assert.deepEqual([signed(2), signed(0), signed(-1)], ['+2', '+0', '−1']);
});

test('a command is followed to its end', () => {
  assert.equal(commandId({ commandId: 'c1', result: {} }), 'c1');
  assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['x', 3] } }), { status: 'rejected', violations: ['x'] });
  assert.deepEqual(['applied', 'rejected', 'cancelled', 'expired', 'queued'].map(isFinal), [true, true, true, true, false]);
  assert.equal(reasons(['to-create-leaves-once-created', 'other']), 'cet élément est déjà créé, other');
});
