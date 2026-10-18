import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, dateLabel, firstName, isNext, newKey, initialForm, nextOrder, parseCharacters, parseSessions, payload, playerCharacters, reasons, settled, statusLabel, statusTone } from '../src/model.ts';

const session = (extra: Record<string, unknown>) => ({ id: 's', order: 1, title: 'T', date: '2026-09-05', status: 'closed', present: [], summary: null, ...extra });
const people = [
  { id: 'elowen', name: 'Elowen Ventargent', kind: 'pj' },
  { id: 'morlox', name: 'Morlox', kind: 'pnj' },
  { id: 'kael', name: 'Kael', kind: 'pj' },
];

test('the sessions come in the order of their position, and what is not a session is dropped', () => {
  const sessions = parseSessions([session({ id: 'b', order: 2 }), 'x', { title: 'no id' }, session({ id: 'a', order: 1 })]);
  assert.deepEqual(sessions.map((s) => s.id), ['a', 'b']);
  assert.deepEqual(parseSessions(undefined), []);
});

test('a session in preparation has no summary and no label to show, and keeps what it is given', () => {
  const [one] = parseSessions([session({ status: 'prep', date: null, label: 'En préparation', present: ['kael', 7] })]);
  assert.deepEqual(one, { id: 's', order: 1, title: 'T', date: null, status: 'prep', label: 'En préparation', present: ['kael'], summary: null });
});

test('only the PCs can be ticked present (present-only-pj)', () => {
  assert.deepEqual(playerCharacters(parseCharacters(people)).map((c) => c.id), ['elowen', 'kael']);
});

test('a present PC is shown by its first name, an unknown id as it is', () => {
  const characters = parseCharacters(people);
  assert.equal(firstName(characters, 'elowen'), 'Elowen');
  assert.equal(firstName(characters, 'ghost'), 'ghost');
});

test('the status is the Capability’s label, else the words of the status; a played or closed session is a success', () => {
  const [prep, live, closed, played, labelled] = parseSessions([
    session({ order: 1, status: 'prep' }),
    session({ order: 2, status: 'live' }),
    session({ order: 3, status: 'closed' }),
    session({ order: 4, status: 'played' }),
    session({ order: 5, status: 'closed', label: 'Terminée' }),
  ]);
  assert.deepEqual([prep, live, closed, played, labelled].map((s) => statusLabel(s!)), ['En préparation', 'En direct', 'Jouée', 'Jouée', 'Terminée']);
  assert.deepEqual([prep, live, closed, played].map((s) => statusTone(s!)), ['accent', 'primary', 'success', 'success']);
});

test('the next session takes the number after the last, and the form proposes it with every PC ticked', () => {
  const sessions = parseSessions([session({ order: 1 }), session({ id: 'z', order: 4 })]);
  assert.equal(nextOrder(sessions), 5);
  assert.equal(nextOrder([]), 1);
  assert.deepEqual(initialForm(sessions, parseCharacters(people)), { title: 'Séance 5', date: '', present: ['elowen', 'kael'] });
});

test('the payload carries the campaign, the title, the date when there is one, and the ticked PCs', () => {
  const sessions = parseSessions([session({ order: 3 })]);
  assert.deepEqual(payload('c1', { title: '  Le gué  ', date: '2026-10-31', present: ['elowen', 'kael'] }, sessions), {
    campaign: 'c1',
    title: 'Le gué',
    plannedOn: '2026-10-31',
    present: ['elowen', 'kael'],
  });
  // A title left blank takes the proposed one; a date left blank is not sent.
  assert.deepEqual(payload('c1', { title: ' ', date: '', present: [] }, sessions), { campaign: 'c1', title: 'Séance 4', present: [] });
});

test('the command names its DataCapability by identifier and version, as the DM’s own', () => {
  assert.deepEqual(commandBody('session.createSession', { a: 1 }, 'k-1'), { dataCapability: 'session.createSession@1', payload: { a: 1 }, caller: { by: 'dm', from: 'SessionList', roles: [] }, idempotencyKey: 'k-1' });
});

test('a live session is drawn like the one in preparation, a date with a time is read as its day', () => {
  const [live] = parseSessions([session({ status: 'live' })]);
  assert.equal(isNext(live!), true);
  assert.equal(dateLabel('2026-09-05T00:00:00Z'), dateLabel('2026-09-05'));
  assert.equal(dateLabel(null), '');
  assert.equal(dateLabel('not a date'), 'not a date');
});

test('each command has a key of its own', () => {
  assert.notEqual(newKey(), newKey());
});

test('a rejection is told in words, an unknown reason as it is', () => {
  assert.equal(reasons(['present-only-pj', 'oneshot-single-session', 'other']), 'seuls les PJ de la campagne peuvent être présents, un one-shot n’a qu’une seule séance, other');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['x'] } }), { status: 'rejected', violations: ['x'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
});
