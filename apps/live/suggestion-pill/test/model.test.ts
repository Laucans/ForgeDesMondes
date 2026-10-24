import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, hostType, parseProps, parseSuggestions, reasons, settled, timeOf } from '../src/model.ts';

const at = (minute: number): string => new Date(Date.UTC(2026, 9, 17, 20, minute)).toISOString();
const one = (n: number, extra: Record<string, unknown> = {}) => ({ id: `e${n}`, kind: 'entity', type: 'pnj', name: `PNJ ${n}`, via: `via ${n}`, detectedAt: at(n), ...extra });

test('the pile is the 5 most recent, the history every suggestion, both newest first whatever the order they come in', () => {
  const eight = [3, 8, 1, 5, 2, 7, 4, 6].map((n) => one(n));
  const { stack, history } = parseSuggestions({ suggestions: [], history: eight });
  assert.deepEqual(history.map((s) => s.id), ['e8', 'e7', 'e6', 'e5', 'e4', 'e3', 'e2', 'e1']);
  assert.deepEqual(stack.map((s) => s.id), ['e8', 'e7', 'e6', 'e5', 'e4']);
});

test('the history is read before the pile, and a bare list is read as the history', () => {
  const four = [1, 2, 3, 4].map((n) => one(n));
  assert.equal(parseSuggestions({ suggestions: four.slice(0, 2), history: four }).history.length, 4);
  assert.equal(parseSuggestions({ suggestions: four.slice(0, 2), history: [] }).history.length, 2);
  assert.equal(parseSuggestions(four).history.length, 4);
});

test('an answer with no time for one suggestion keeps the order it came in', () => {
  const given = [one(1, { detectedAt: '' }), one(9), one(5)];
  assert.deepEqual(parseSuggestions(given).history.map((s) => s.id), ['e1', 'e9', 'e5']);
});

test('what is not a suggestion is left out, and a sound carries no type', () => {
  const { history } = parseSuggestions([null, 3, { name: 'no id' }, { id: 's1', kind: 'sound', type: 'pnj', name: 'Porte', via: 'la porte' }, { id: 'e1', kind: 'other' }]);
  assert.deepEqual(history.map((s) => [s.id, s.kind, s.type]), [['s1', 'sound', ''], ['e1', 'entity', '']]);
  assert.deepEqual(parseSuggestions(undefined), { stack: [], history: [] });
  assert.deepEqual(parseSuggestions('nope'), { stack: [], history: [] });
});

test('the time is the hour and minute, or nothing', () => {
  assert.match(timeOf(at(5)), /^\d{2}:\d{2}$/);
  assert.equal(timeOf(''), '');
  assert.equal(timeOf('not a date'), '');
});

test('the host is told a place, an event or a character', () => {
  assert.equal(hostType('lieu'), 'place');
  assert.equal(hostType('Place'), 'place');
  assert.equal(hostType('événement'), 'event');
  assert.equal(hostType('evenement'), 'event');
  assert.equal(hostType('pnj'), 'character');
  assert.equal(hostType('pj'), 'character');
  assert.equal(hostType(''), 'character');
});

test('a command names its DataCapability and version, the DM as caller, and a target only when it has one', () => {
  assert.deepEqual(commandBody('live.setMicrophone', { on: true }), { dataCapability: 'live.setMicrophone@1', payload: { on: true }, caller: { by: 'dm', from: 'SuggestionPill', roles: [] } });
  assert.deepEqual(commandBody('soundbox.playSound', { source: 'suggestion' }, 'snd-porte'), {
    dataCapability: 'soundbox.playSound@1',
    payload: { source: 'suggestion' },
    target: { id: 'snd-porte' },
    caller: { by: 'dm', from: 'SuggestionPill', roles: [] },
  });
});

test('a command settles as the server says, and its refusal is told in words', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['status-lifecycle', 7] } }), { status: 'rejected', violations: ['status-lifecycle'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(reasons(['status-lifecycle', 'other-rule']), 'la séance est terminée, other-rule');
});

test('the campaign is the only prop', () => {
  assert.deepEqual(parseProps({ campaign: 'c1', extra: 1 }), { campaign: 'c1' });
  assert.deepEqual(parseProps({ campaign: 4 }), { campaign: '' });
});
