import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, commandId, createLabel, isFinal, parseNotes, pending, reasons, settled } from '../src/model.ts';

test('a probable duplicate starts unchecked, whatever the answer says', () => {
  const notes = parseNotes({ imports: [], proposals: [
    { id: 'a', type: 'pnj', name: 'A', excerpt: 'x', existing: 'gaspard', checked: true },
    { id: 'b', type: 'lieu', name: 'B', excerpt: 'y', existing: null, checked: true },
    { id: 'c', type: 'lieu', name: 'C', excerpt: 'z', existing: null, checked: false },
  ] });
  assert.deepEqual(notes.proposals.map((p) => p.checked), [false, true, false]);
});

test('the answer is read as { imports, proposals } or as the bare list of imports, and junk reads as empty', () => {
  const entry = { id: 'imp-1', date: '2026-09-14', title: 'T', chars: 10, proposed: 4, created: 3 };
  assert.deepEqual(parseNotes([entry]).imports, [entry]);
  assert.deepEqual(parseNotes({ imports: [entry], proposals: [] }).imports, [entry]);
  assert.deepEqual(parseNotes(undefined), { imports: [], proposals: [] });
  assert.deepEqual(parseNotes({ imports: [{ title: 'no id' }, 3], proposals: 'x' }), { imports: [], proposals: [] });
});

test('a proposal the DM already settled is not offered again', () => {
  const notes = parseNotes({ proposals: [{ id: 'a' }, { id: 'b' }] });
  assert.deepEqual(pending(notes, new Set(['a'])).map((p) => p.id), ['b']);
});

test('a command names its DataCapability with its version, as the DM', () => {
  assert.deepEqual(commandBody('ai.extractFromNotes', { campaign: 'c', text: 't' }), {
    dataCapability: 'ai.extractFromNotes@1',
    payload: { campaign: 'c', text: 't' },
    caller: { by: 'dm', from: 'NoteImport', roles: [] },
  });
});

test('the server\'s answers are read for their status, violations and command id', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['text-only-payload', 3] } }), { status: 'rejected', violations: ['text-only-payload'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(commandId({ commandId: 'c1' }), 'c1');
  assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
  assert.equal(isFinal('applied') && isFinal('rejected') && !isFinal('queued'), true);
  assert.equal(reasons(['text-only-payload', 'other']), 'seul du texte est accepté, other');
});

test('the create button says how many it creates', () => {
  assert.equal(createLabel(0), 'Créer la sélection (0)');
  assert.equal(createLabel(4), 'Créer la sélection (4)');
});
