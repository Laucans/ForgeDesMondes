import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chipKind, heading, meta, parseJournal, parseSummary } from '../src/model.ts';

const answer = {
  session: { id: 's3', order: 3, title: 'Le marché noir', date: '2026-10-03' },
  summary: 'Le groupe a suivi la piste.',
  summaryMeta: 'Généré à la clôture par qwen',
  mentions: [{ id: 'morlox', type: 'pnj', name: 'Morlox' }, { id: '', type: 'pnj', name: 'sans id' }, 'x'],
  threads: ['Un fil', 3, 'Un autre'],
};

test('a summary answer is read whole, what is malformed in it left out', () => {
  const summary = parseSummary(answer);
  assert.equal(summary?.session.id, 's3');
  assert.deepEqual(summary?.mentions, [{ id: 'morlox', type: 'pnj', name: 'Morlox' }]);
  assert.deepEqual(summary?.threads, ['Un fil', 'Un autre']);
});

test('no summary, or no session, is « no closed session yet », not an error', () => {
  assert.equal(parseSummary(undefined), null);
  assert.equal(parseSummary({}), null);
  assert.equal(parseSummary({ ...answer, session: null }), null);
});

test('a closed session whose summary is blank is still a session', () => {
  assert.equal(parseSummary({ ...answer, summary: '' })?.summary, '');
});

test('the heading carries the number and the title; the meta the date, then the source', () => {
  const summary = parseSummary(answer);
  assert.ok(summary);
  assert.equal(heading(summary), 'Séance 3 — Le marché noir');
  assert.match(meta(summary), /^samedi 3 octobre 2026 · Généré à la clôture par qwen$/);
  assert.equal(meta({ ...summary, summaryMeta: '' }), 'samedi 3 octobre 2026');
  assert.equal(meta({ ...summary, session: { ...summary.session, date: 'bientôt' } }), 'bientôt · Généré à la clôture par qwen');
});

test('the journal keeps its lines in order, each with its time and text', () => {
  const journal = parseJournal({ journal: [{ t: '20:04', text: 'a' }, { t: '20:06', text: '' }, null, { t: '20:11', text: 'b' }], journalStats: '3 h' });
  assert.deepEqual(journal, { journal: [{ t: '20:04', text: 'a' }, { t: '20:11', text: 'b' }], journalStats: '3 h' });
  assert.deepEqual(parseJournal(undefined), { journal: [], journalStats: '' });
});

test('a mention is coloured as a character, a place, or a medium', () => {
  assert.deepEqual(['pnj', 'lieu', 'événement', ''].map(chipKind), ['pnj', 'lieu', 'media', 'media']);
});
