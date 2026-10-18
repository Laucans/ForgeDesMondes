import assert from 'node:assert/strict';
import { test } from 'node:test';
import { defaultTitle, parseCharacters, parseSessions, payload, pcs, presentName, problem, settled } from '../src/model.ts';

const cast = parseCharacters([
  { id: 'elowen', name: 'Elowen Ventargent', kind: 'pj' },
  { id: 'morlox', name: 'Morlox', kind: 'pnj' },
  'not a character',
]);

test('only PCs are offered, and only PCs are sent as present (present-only-pj)', () => {
  assert.deepEqual(pcs(cast).map((c) => c.id), ['elowen']);
  const body = payload('c1', { title: ' T ', plannedOn: '2026-11-01', present: ['elowen', 'morlox', 'ghost'] }, cast);
  assert.deepEqual(body, { campaign: 'c1', title: 'T', plannedOn: '2026-11-01', present: ['elowen'] });
});

test('sessions come by number whatever the answer, and an answer that is not a list is no session', () => {
  const sessions = parseSessions([{ id: 'b', order: 2, title: 'B' }, { id: 'a', order: 1, title: 'A', date: '', summary: '' }]);
  assert.deepEqual(sessions.map((s) => s.id), ['a', 'b']);
  assert.equal(sessions[0]?.date, null);
  assert.equal(sessions[0]?.summary, null);
  assert.deepEqual(parseSessions({ error: 'x' }), []);
  assert.equal(defaultTitle(sessions), 'Séance 3');
  assert.equal(defaultTitle([]), 'Séance 1');
});

test('the first name stands for a PC, the id for one the list does not know', () => {
  assert.equal(presentName(cast, 'elowen'), 'Elowen');
  assert.equal(presentName(cast, 'inconnu'), 'inconnu');
});

test('a form needs a title and a date', () => {
  assert.match(problem({ title: ' ', plannedOn: '2026-11-01', present: [] }), /titre/);
  assert.match(problem({ title: 'T', plannedOn: '', present: [] }), /date/);
  assert.equal(problem({ title: 'T', plannedOn: '2026-11-01', present: [] }), '');
});

test('a command answer is read whatever its shape', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['present-only-pj'] } }), { status: 'rejected', violations: ['present-only-pj'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
});
