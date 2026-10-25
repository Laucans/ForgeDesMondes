import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount } from '../src/index.ts';
import type { Journal, Shell, Summary } from '../src/types.ts';

const fixture = <T>(name: string): T => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8')) as T;
const summary = fixture<Summary>('session.getPreviousSessionSummary');
const journal = fixture<Journal>('session.getSessionJournal');

interface Read {
  id: string;
  sha256: string | undefined;
  variables: Record<string, unknown> | undefined;
}

/** A shell over the mock-up's fixtures; it reads, and the type of `Shell` lets it do nothing else. */
function fakeShell(answers: Record<string, () => unknown> = {}) {
  const reads: Read[] = [];
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      const data = answers[id] ? answers[id]() : id === 'session.getPreviousSessionSummary' ? summary : journal;
      return { asOf: 1, data };
    },
  };
  return { shell, reads };
}

async function card(shell: Shell, props: Record<string, unknown> = { session: 's4' }) {
  const { window } = new JSDOM('<!doctype html><body><div id="host"></div></body>');
  const host = window.document.getElementById('host') as HTMLElement;
  const mounted = mount(host, props, shell);
  await new Promise((resolve) => setTimeout(resolve, 0));
  return { host, mounted, document: window.document, click: (id: string) => host.querySelector<HTMLElement>(`#${id}`)?.click() };
}

test('the card shows the previous session’s number, title, date, summary and source', async () => {
  const { host } = await card(fakeShell().shell);
  assert.equal(host.querySelector('#previous-title')?.textContent, `Séance ${summary.session.order} — ${summary.session.title}`);
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, summary.summary);
  assert.equal(host.querySelector('#previous-meta')?.textContent, `samedi 3 octobre 2026 · ${summary.summaryMeta}`);
  assert.equal(host.querySelectorAll('#previous-mentions-list a').length, summary.mentions.length);
  assert.equal(host.querySelectorAll('#previous-threads-list li').length, summary.threads.length);
});

test('the journal stays closed and unread until « Journal complet » is clicked', async () => {
  const { shell, reads } = fakeShell();
  const { host, click } = await card(shell);
  const panel = host.querySelector<HTMLElement>('aside#previous-journal');
  assert.equal(panel?.hidden, true);
  assert.deepEqual(reads.map((r) => r.id), ['session.getPreviousSessionSummary']);

  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(panel?.hidden, false);
  assert.equal(panel?.getAttribute('aria-labelledby'), 'previous-journal-title');
  assert.equal(host.querySelector('#previous-journal-title')?.textContent, 'Journal transcrit');
  const rows = [...host.querySelectorAll('#previous-journal-lines li')];
  assert.equal(rows.length, journal.journal.length);
  journal.journal.forEach((line, i) => {
    assert.equal(rows[i]?.querySelector('time')?.textContent, line.t);
    assert.equal(rows[i]?.querySelector('span')?.textContent, line.text);
  });
  assert.equal(host.querySelector('#previous-journal-meta')?.textContent, journal.journalStats);
  // The journal is asked for the summary’s session, by identifier alone.
  assert.deepEqual(reads[1], { id: 'session.getSessionJournal', sha256: undefined, variables: { session: summary.session.id } });
});

test('closing the panel and reopening it does not read the journal again', async () => {
  const { shell, reads } = fakeShell();
  const { host, click } = await card(shell);
  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  click('previous-journal-close');
  assert.equal(host.querySelector<HTMLElement>('#previous-journal')?.hidden, true);
  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(reads.filter((r) => r.id === 'session.getSessionJournal').length, 1);
});

test('every part that shows a read names its Capability', async () => {
  const { host } = await card(fakeShell().shell);
  const named = new Set([...host.querySelectorAll('[data-reads]')].map((e) => e.getAttribute('data-reads')));
  assert.deepEqual([...named].sort(), ['session.getPreviousSessionSummary', 'session.getSessionJournal']);
  assert.equal(host.querySelectorAll('[data-writes]').length, 0);
});

test('with no closed session the card says so, and the journal cannot be opened', async () => {
  const { shell, reads } = fakeShell({ 'session.getPreviousSessionSummary': () => ({}) });
  const { host, click } = await card(shell);
  assert.match(host.querySelector('#previous-summary-text')?.textContent ?? '', /Aucune séance précédente/);
  click('previous-journal-open');
  assert.equal(reads.length, 1);
});

test('a journal that cannot be read says so, and is asked for again at the next opening', async () => {
  let fail = true;
  const { shell, reads } = fakeShell({
    'session.getSessionJournal': () => {
      if (fail) throw new Error('down');
      return journal;
    },
  });
  const { host, click } = await card(shell);
  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(host.querySelector('#previous-journal-meta')?.textContent, 'Impossible de lire le journal.');
  fail = false;
  click('previous-journal-close');
  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(host.querySelectorAll('#previous-journal-lines li').length, journal.journal.length);
  assert.equal(reads.filter((r) => r.id === 'session.getSessionJournal').length, 2);
});

test('a closed session with no summary text keeps its heading and its journal', async () => {
  const { shell } = fakeShell({ 'session.getPreviousSessionSummary': () => ({ ...summary, summary: '' }) });
  const { host } = await card(shell);
  assert.equal(host.querySelector('#previous-title')?.textContent, `Séance ${summary.session.order} — ${summary.session.title}`);
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, 'Cette séance n’a pas de résumé.');
  assert.equal(host.querySelector<HTMLButtonElement>('#previous-journal-open')?.disabled, false);
});

test('a summary that cannot be read, or drawn, says so instead of waiting for ever', async () => {
  const { host } = await card(fakeShell({ 'session.getPreviousSessionSummary': () => { throw new Error('down'); } }).shell);
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, 'Impossible de lire la séance précédente.');
});

test('an empty journal says it is empty', async () => {
  const { host, click } = await card(fakeShell({ 'session.getSessionJournal': () => ({ journal: [], journalStats: '' }) }).shell);
  click('previous-journal-open');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(host.querySelector('#previous-journal-meta')?.textContent, 'Le journal de cette séance est vide.');
  assert.equal(host.querySelectorAll('#previous-journal-lines li').length, 0);
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

test('the summary is read for the session the card is given', async () => {
  const { shell, reads } = fakeShell();
  const { host } = await card(shell, { session: 's4' });
  assert.deepEqual(reads, [{ id: 'session.getPreviousSessionSummary', sha256: undefined, variables: { session: 's4' } }]);
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, summary.summary);
});

test('with no session given nothing is read, and the card says there is no closed session', async () => {
  for (const props of [{}, { session: '' }, { session: 4 }]) {
    const { shell, reads } = fakeShell();
    const { host, click } = await card(shell, props);
    assert.equal(reads.length, 0);
    assert.equal(host.querySelector('#previous-summary-text')?.textContent, 'Aucune séance précédente n’est encore close.');
    assert.equal(host.querySelector<HTMLButtonElement>('#previous-journal-open')?.disabled, true);
    click('previous-journal-open');
    assert.equal(reads.length, 0);
  }
});

test('a session that arrives later, or changes, is read again; the same one is not', async () => {
  const other = { ...summary, session: { ...summary.session, id: 's2', order: 2, title: 'Avant' } };
  const { shell, reads } = fakeShell({
    'session.getPreviousSessionSummary': () => (reads.at(-1)?.variables?.session === 's5' ? other : summary),
  });
  const { host, mounted, click } = await card(shell, {});
  assert.equal(reads.length, 0);

  mounted.update({ session: 's4' });
  await settle();
  assert.equal(host.querySelector('#previous-title')?.textContent, `Séance ${summary.session.order} — ${summary.session.title}`);
  mounted.update({ session: 's4' });
  await settle();
  assert.equal(reads.length, 1);

  click('previous-journal-open');
  await settle();
  assert.equal(host.querySelectorAll('#previous-journal-lines li').length, journal.journal.length);

  mounted.update({ session: 's5' });
  await settle();
  assert.deepEqual(reads.at(-1)?.variables, { session: 's5' });
  assert.equal(host.querySelector('#previous-title')?.textContent, 'Séance 2 — Avant');
  // The last session’s journal is gone, and the panel closed, until it is opened for this one.
  assert.equal(host.querySelector<HTMLElement>('#previous-journal')?.hidden, true);
  assert.equal(host.querySelectorAll('#previous-journal-lines li').length, 0);

  mounted.update({});
  await settle();
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, 'Aucune séance précédente n’est encore close.');
});

test('an answer to a session the card has left is not drawn', async () => {
  let release: () => void = () => undefined;
  const slow = new Promise<void>((resolve) => (release = resolve));
  const { shell } = fakeShell();
  const reading = shell.capability.bind(shell);
  const late: Shell = {
    async capability(id, sha256, variables) {
      if (variables?.session === 's4') await slow;
      return reading(id, sha256, variables);
    },
  };
  const { host, mounted } = await card(late, { session: 's4' });
  mounted.update({});
  release();
  await settle();
  assert.equal(host.querySelector('#previous-summary-text')?.textContent, 'Aucune séance précédente n’est encore close.');
});
