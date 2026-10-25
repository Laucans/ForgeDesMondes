import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const fixture = <T>(name: string): T => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8')) as T;
const bar = fixture<{ scenes: unknown[]; currentScene: number }>('LiveSessionBar');
const live = { ...fixture<Record<string, unknown>>('session.getLiveSession'), scenes: bar.scenes, currentScene: bar.currentScene };
const journal = fixture<unknown>('session.getSessionJournal');
const summary = fixture<Record<string, unknown>>('session.getPreviousSessionSummary');
const onIllustration = { base: { kind: 'illustration', title: 'Le sceau' }, window: { title: 'Lettre' }, view: null };

/** Runs at once and at 23:29, so the session of the fixture (20:02) lasted 3 h 27 min. */
const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5, now: () => new Date(2026, 9, 17, 23, 29) };

/** A shell over the fixtures that records what the bar asked and sent. */
function fakeShell(options: { diffusion?: unknown; status?: string; violations?: string[]; failRecap?: boolean } = {}) {
  const reads: { id: string; variables: Record<string, unknown> | undefined }[] = [];
  const sent: Record<string, unknown>[] = [];
  const contexts: [string, unknown][] = [];
  let diffusion = options.diffusion ?? fixture<unknown>('live.getDiffusion');
  const shell: Shell = {
    async capability(id, _sha256, variables) {
      reads.push({ id, variables });
      if (options.failRecap && (id === 'session.getSessionJournal' || id === 'session.getPreviousSessionSummary')) throw new Error('down');
      const data = id === 'session.getLiveSession' ? live : id === 'live.getDiffusion' ? diffusion : id === 'session.getSessionJournal' ? journal : summary;
      return { asOf: 1, data };
    },
    async command(body) {
      const command = body as Record<string, unknown>;
      sent.push(command);
      // Showing the cover is what the server then answers `live.getDiffusion` with.
      if (String(command.dataCapability).startsWith('live.showCover')) diffusion = { base: { kind: 'cover', title: '' }, window: null, view: null };
      return { commandId: 'c1', result: { commandId: 'c1', status: options.status ?? 'applied', violations: options.violations ?? [] } };
    },
    async commandResult() {
      return { result: { status: 'applied', violations: [] } };
    },
    onDataVersion: () => () => undefined,
    context: { set: (name, value) => void contexts.push([name, value]) },
  };
  return { shell, reads, sent, contexts };
}

async function render(shell: Shell, props: Record<string, unknown> = {}) {
  const { window } = new JSDOM('<!doctype html><body><div id="host"></div></body>', { url: 'http://localhost/' });
  const host = window.document.getElementById('host') as HTMLElement;
  mount(host, props, shell, timing);
  await settle();
  return { host, window, click: async (id: string) => { host.querySelector<HTMLElement>(`#${id}`)?.click(); await settle(); } };
}

test('the live session is read by the id the page names, else as the one running', async () => {
  const named = fakeShell();
  await render(named.shell, { session: 's9' });
  assert.deepEqual(named.reads.find((r) => r.id === 'session.getLiveSession')?.variables, { id: 's9' });
  const running = fakeShell();
  await render(running.shell, { session: '' });
  assert.equal(running.reads.find((r) => r.id === 'session.getLiveSession')?.variables, undefined);
});

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 5));

test('the bar shows the session’s number, title, start and scenes, the current one marked', async () => {
  const { host } = await render(fakeShell().shell);
  assert.equal(host.querySelector('#session-bar-title')?.textContent, 'Séance 4 — L’embuscade du Gué');
  assert.equal(host.querySelector('#session-bar-meta')?.textContent, 'En direct depuis 20:02');
  const chips = [...host.querySelectorAll('#session-bar-scenes button')];
  assert.equal(chips.length, 3);
  assert.equal(chips[0]?.textContent, 'Scène 1 · Retour à la Taverne du Sanglier');
  assert.deepEqual(chips.map((c) => c.classList.contains('is-on')), [true, false, false]);
  assert.equal(host.querySelector('#session-bar-showing')?.textContent, 'Les joueurs voient la couverture de la campagne');
});

test('the current scene is published to the screen, and follows a click', async () => {
  const { shell, contexts } = fakeShell();
  const { host, click } = await render(shell);
  await click('session-bar-scene-3');
  assert.deepEqual(contexts, [['scene', 0], ['scene', 2]]);
  assert.equal(host.querySelector('#session-bar-scene-3')?.classList.contains('is-on'), true);
});

test('on the cover, no button undoes the diffusion; elsewhere the cover and window buttons are offered', async () => {
  const onCover = await render(fakeShell().shell);
  assert.equal(onCover.host.querySelector('#session-bar-cover'), null);
  assert.equal(onCover.host.querySelector('#session-bar-close-window'), null);
  const away = await render(fakeShell({ diffusion: onIllustration }).shell);
  assert.equal(away.host.querySelector('#session-bar-cover')?.getAttribute('data-writes'), 'live.showCover');
  assert.equal(away.host.querySelector('#session-bar-close-window')?.getAttribute('data-writes'), 'live.closeWindow');
  assert.match(away.host.querySelector('#session-bar-showing')?.textContent ?? '', /« Le sceau » \(scène\).*fenêtre « Lettre »/);
});

test('the cover button submits live.showCover by identifier, then the bar shows the cover again', async () => {
  const { shell, sent } = fakeShell({ diffusion: onIllustration });
  const { host, click } = await render(shell);
  await click('session-bar-cover');
  assert.deepEqual(sent.map((c) => c.dataCapability), ['live.showCover@1']);
  assert.equal(host.querySelector('#session-bar-cover'), null);
  assert.equal(host.querySelector('#session-bar-showing')?.textContent, 'Les joueurs voient la couverture de la campagne');
});

test('the close-window button submits live.closeWindow, and nothing else', async () => {
  const { shell, sent } = fakeShell({ diffusion: onIllustration });
  const { click } = await render(shell);
  await click('session-bar-close-window');
  assert.deepEqual(sent.map((c) => c.dataCapability), ['live.closeWindow@1']);
});

test('« Clore la séance » only asks: nothing is sent until the dialog’s own button', async () => {
  const { shell, sent } = fakeShell();
  const { host, click } = await render(shell);
  await click('session-bar-close');
  assert.equal(sent.length, 0);
  assert.equal(host.querySelector<HTMLElement>('#session-close-modal')?.hidden, false);
  assert.equal(host.querySelector('#session-close-title')?.textContent, 'Clore la séance 4');
  await click('session-close-cancel');
  assert.equal(host.querySelector<HTMLElement>('#session-close-modal')?.hidden, true);
  assert.equal(sent.length, 0);
});

test('closing submits session.closeSession on its session, then the recap shows lines, duration, model and summary', async () => {
  const { shell, sent, reads } = fakeShell();
  const { host, click } = await render(shell);
  await click('session-bar-close');
  await click('session-close-submit');
  assert.equal(sent.length, 1);
  assert.equal(sent[0]?.dataCapability, 'session.closeSession@1');
  assert.deepEqual(sent[0]?.target, { aggregate: 'Session', id: 's4' });
  // The journal and the summary are read for the session just closed.
  assert.deepEqual(reads.slice(-2).map((r) => [r.id, r.variables]), [['session.getSessionJournal', { session: 's4' }], ['session.getPreviousSessionSummary', { session: 's4' }]]);
  assert.equal(host.querySelector('#session-close-title')?.textContent, 'Séance 4 close');
  assert.match(host.querySelector('#session-close-lines')?.textContent ?? '', /14 lignes/);
  assert.match(host.querySelector('#session-close-duration')?.textContent ?? '', /3 h 27 min/);
  assert.equal(host.querySelector('#session-close-model strong')?.textContent, 'qwen2.5-14b-instruct (opencode maison)');
  assert.equal(host.querySelector('#session-close-summary')?.textContent, summary.summary);
  // A closed session is not reopened from the bar.
  assert.equal(host.querySelector<HTMLButtonElement>('#session-bar-close')?.disabled, true);
  assert.equal(host.querySelector('#session-bar-badges')?.textContent, 'Close');
});

test('a refused close says why and can be tried again; nothing is recapped', async () => {
  const { shell, sent } = fakeShell({ status: 'rejected', violations: ['status-lifecycle'] });
  const { host, click } = await render(shell);
  await click('session-bar-close');
  await click('session-close-submit');
  assert.match(host.querySelector('#session-close-status')?.textContent ?? '', /cette séance est déjà close/);
  assert.equal(host.querySelector('#session-close-lines'), null);
  assert.equal(host.querySelector<HTMLButtonElement>('#session-bar-close')?.disabled, false);
  await click('session-close-submit');
  assert.equal(sent.length, 2);
  // Each attempt has a key of its own: a retry is not answered with the refusal before it.
  assert.notEqual(sent[0]?.idempotencyKey, sent[1]?.idempotencyKey);
});

test('a command that did not apply is said, and the diffusion is not shown as changed', async () => {
  const { shell } = fakeShell({ diffusion: onIllustration, status: 'expired' });
  const { host, click } = await render(shell);
  await click('session-bar-cover');
  assert.match(host.querySelector('#session-bar-showing')?.textContent ?? '', /n’a pas abouti \(état : expired\)/);
});

test('a close that was applied is a closed session, even when the recap cannot be drawn in full', async () => {
  const { host, click } = await render(fakeShell({ failRecap: true }).shell);
  await click('session-bar-close');
  await click('session-close-submit');
  assert.equal(host.querySelector<HTMLButtonElement>('#session-bar-close')?.disabled, true);
  assert.equal(host.querySelector('#session-close-submit'), null);
});

test('the dialog takes the focus, and Escape leaves it when nothing is being sent', async () => {
  const { host, window, click } = await render(fakeShell().shell);
  await click('session-bar-close');
  assert.equal(window.document.activeElement?.id, 'session-close-cancel');
  host.querySelector('#session-close-cancel')?.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(host.querySelector<HTMLElement>('#session-close-modal')?.hidden, true);
});

test('a recap that cannot be read in full still tells the session is closed', async () => {
  const { host, click } = await render(fakeShell({ failRecap: true }).shell);
  await click('session-bar-close');
  await click('session-close-submit');
  assert.equal(host.querySelector('#session-close-title')?.textContent, 'Séance 4 close');
  assert.match(host.querySelector('#session-close-status')?.textContent ?? '', /n’a pas pu être lu/);
  assert.equal(host.querySelector('#session-bar-badges')?.textContent, 'Close');
});

test('every actuator names the DataCapability it writes and every part a Capability it reads', async () => {
  const { host } = await render(fakeShell({ diffusion: onIllustration }).shell);
  const writes = new Set([...host.querySelectorAll('[data-writes]')].map((e) => e.getAttribute('data-writes')));
  const reads = new Set([...host.querySelectorAll('[data-reads]')].map((e) => e.getAttribute('data-reads')));
  assert.deepEqual([...writes].sort(), ['live.closeWindow', 'live.showCover', 'session.closeSession']);
  assert.ok(reads.has('session.getLiveSession') && reads.has('live.getDiffusion'));
});
