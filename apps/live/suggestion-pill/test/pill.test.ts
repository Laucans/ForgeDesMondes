import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const manifest = JSON.parse(readFileSync(new URL('../micro-ui.json', import.meta.url), 'utf8')) as { needs: string[]; actions: string[] };

const at = (minute: number): string => new Date(Date.UTC(2026, 9, 17, 20, minute)).toISOString();
const entity = (n: number, type = 'pnj') => ({ id: `e${n}`, kind: 'entity', type, name: `PNJ ${n}`, via: `via ${n}`, detectedAt: at(n) });
const sound = (n: number) => ({ id: `s${n}`, kind: 'sound', name: `Son ${n}`, via: `bruit ${n}`, detectedAt: at(n) });

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  target?: { id: string };
  caller: { by: string; from: string };
}

interface Options {
  /** What live.listSuggestions answers; a function is asked at each read. */
  answer?: unknown;
  /** What the server answers each command, in order; the last answer repeats. */
  status?: string | string[];
  violations?: string[];
  failRead?: () => boolean;
  failCommand?: boolean;
  /** Keeps every command waiting until it is released. */
  hold?: Promise<void>;
}

/** A shell over a fixed answer: reads answer it, a command is recorded and settles as told. */
function fakeShell(options: Options = {}) {
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables?: Record<string, unknown> }> = [];
  const listeners = new Set<(dataVersion: number) => void>();
  const stops: number[] = [];
  const statusOf = (n: number): string => (Array.isArray(options.status) ? (options.status[n - 1] ?? options.status.at(-1)) : options.status) ?? 'applied';
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      if (options.failRead?.()) throw new Error('the server is not there');
      const answer = typeof options.answer === 'function' ? (options.answer as () => unknown)() : options.answer;
      return { asOf: 1, data: answer ?? { suggestions: [], history: [] } };
    },
    async command(body) {
      if (options.failCommand) throw new Error('the server is not there');
      sent.push(body as Sent);
      await options.hold;
      return { commandId: `c${sent.length}`, result: { status: statusOf(sent.length), violations: options.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: statusOf(Number(commandId.slice(1))), violations: options.violations ?? [] } };
    },
    onDataVersion(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        stops.push(1);
      };
    },
  };
  return { shell, sent, reads, listeners, stops, moveOn: () => listeners.forEach((l) => l(2)) };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 3, watchMs: 1 };

const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function pill(props: Record<string, unknown> = {}, options?: Options) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="SuggestionPill"></div></body></html>', { url: 'http://localhost/' });
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const opened: Array<{ type: string; id: string }> = [];
  dom.window.document.addEventListener('fdm:open', (event) => opened.push((event as CustomEvent<{ type: string; id: string }>).detail));
  const mounted = mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the pill`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const names = (selector: string): string[] => [...host.querySelectorAll(`${selector} li b`)].map((b) => b.textContent ?? '');
  return { dom, host, ...fake, mounted, opened, $, click, names };
}

const EIGHT = [3, 8, 1, 5, 2, 7, 4, 6];

test('the manifest declares the Capability it reads and the DataCapabilities it triggers, and every one has a part that names it', async () => {
  const p = await pill({}, { answer: { suggestions: [entity(1)], history: [entity(1), sound(2)] } });
  assert.deepEqual(manifest.needs, ['live.listSuggestions']);
  assert.deepEqual(manifest.actions, ['live.setMicrophone', 'soundbox.playSound']);
  for (const need of manifest.needs) assert.ok(p.host.querySelector(`[data-reads="${need}"]`), `a part reads ${need}`);
  for (const action of manifest.actions) assert.ok(p.host.querySelector(`[data-writes="${action}"]`), `a part writes ${action}`);
});

test('the pill is fixed at the bottom right above the other panes', async () => {
  const p = await pill();
  const style = p.dom.window.document.querySelector('style[data-micro-ui="SuggestionPill"]')?.textContent ?? '';
  const rule = /\.mf-pill \{([^}]*)\}/.exec(style)?.[1] ?? '';
  assert.match(rule, /position: fixed/);
  assert.match(rule, /right: var\(--space-3\)/);
  assert.match(rule, /bottom: var\(--space-3\)/);
  assert.match(rule, /z-index: 1000/);
  assert.equal(p.dom.window.document.querySelectorAll('style[data-micro-ui="SuggestionPill"]').length, 1);
});

test('it reads the suggestions by identifier alone, and names the campaign when the screen does', async () => {
  const p = await pill({ campaign: 'c1' });
  assert.deepEqual(p.reads[0], { id: 'live.listSuggestions', sha256: undefined, variables: { campaign: 'c1' } });
  const alone = await pill();
  assert.deepEqual(alone.reads[0]?.variables, {});
});

test('8 suggestions: the stack shows the 5 most recent, newest first', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: EIGHT.map((n) => entity(n)) } });
  assert.deepEqual(p.names('#suggestions-stack'), ['PNJ 8', 'PNJ 7', 'PNJ 6', 'PNJ 5', 'PNJ 4']);
});

test('the history, once opened, lists every suggestion by detection time descending', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: [...EIGHT.map((n) => entity(n)), sound(9)] } });
  const toggle = p.$('#suggestions-history-toggle');
  assert.equal(p.$('#suggestions-history').hidden, true);
  assert.equal(toggle.getAttribute('aria-expanded'), 'false');
  await p.click('#suggestions-history-toggle');
  assert.equal(p.$('#suggestions-history').hidden, false);
  assert.equal(toggle.getAttribute('aria-expanded'), 'true');
  assert.deepEqual(p.names('#suggestions-history-list'), ['Son 9', 'PNJ 8', 'PNJ 7', 'PNJ 6', 'PNJ 5', 'PNJ 4', 'PNJ 3', 'PNJ 2', 'PNJ 1']);
  await p.click('#suggestions-history-toggle');
  assert.equal(p.$('#suggestions-history').hidden, true);
});

test('a pill with nothing heard says so, and the microphone starts off', async () => {
  const p = await pill();
  assert.match(p.$('#suggestions-stack').textContent ?? '', /Activez le micro/);
  assert.match(p.$('#suggestions-history-list').textContent ?? '', /Aucune suggestion/);
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'idle');
  assert.equal(p.$('#suggestions-mic-button').getAttribute('aria-pressed'), 'false');
  assert.equal(p.sent.length, 0, 'nothing is sent, so nothing listens, before the DM clicks');
});

test('the toggle turns the microphone on, then off, with the command each time', async () => {
  const p = await pill();
  await p.click('#suggestions-mic-button');
  assert.equal(p.sent.length, 1);
  assert.equal(p.sent[0]?.dataCapability, 'live.setMicrophone@1');
  assert.deepEqual(p.sent[0]?.payload, { on: true });
  assert.equal(p.sent[0]?.target, undefined);
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'recording');
  assert.equal(p.$('#suggestions-mic-button').getAttribute('aria-pressed'), 'true');
  assert.match(p.$('#suggestions-mic-box').textContent ?? '', /Écoute en local/);
  await p.click('#suggestions-mic-button');
  assert.equal(p.sent.length, 2);
  assert.deepEqual(p.sent[1]?.payload, { on: false });
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'paused');
  assert.equal(p.$('#suggestions-mic-button').getAttribute('aria-pressed'), 'false');
  assert.match(p.$('#suggestions-heard').textContent ?? '', /rien n’est écouté/);
});

test('the button shows the microphone on at once and holds still until the server has answered', async () => {
  let release: () => void = () => undefined;
  const p = await pill({}, { hold: new Promise<void>((resolve) => (release = resolve)) });
  const button = p.$('#suggestions-mic-button') as HTMLButtonElement;
  button.dispatchEvent(new p.dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'recording');
  assert.equal(button.disabled, true);
  button.dispatchEvent(new p.dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(p.sent.length, 1, 'a second click while the first is under way sends nothing');
  release();
  await settle();
  assert.equal(button.disabled, false);
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'recording');
  assert.equal(p.sent.length, 1);
});

test('a refused microphone goes back where it was and says why', async () => {
  const p = await pill({}, { status: 'rejected', violations: ['status-lifecycle'] });
  await p.click('#suggestions-mic-button');
  assert.equal(p.$('#suggestions-mic-box').dataset.state, 'idle');
  assert.equal(p.$('#suggestions-status').hidden, false);
  assert.match(p.$('#suggestions-status').textContent ?? '', /la séance est terminée/);
  assert.equal(p.listeners.size, 0, 'a microphone that did not start is not watched');
});

test('a command that never settles, or never leaves, leaves the microphone as it was', async () => {
  const slow = await pill({}, { status: 'queued' });
  await slow.click('#suggestions-mic-button');
  assert.equal(slow.$('#suggestions-mic-box').dataset.state, 'idle');
  assert.match(slow.$('#suggestions-status').textContent ?? '', /état : queued/);
  const down = await pill({}, { failCommand: true });
  await down.click('#suggestions-mic-button');
  assert.equal(down.$('#suggestions-mic-box').dataset.state, 'idle');
  assert.match(down.$('#suggestions-status').textContent ?? '', /n’a pas pu être envoyée/);
});

test('while the microphone is on the suggestions are read again each time the data moves, and no more once it is off', async () => {
  let heard = [entity(1)];
  const p = await pill({}, { answer: () => ({ suggestions: heard, history: heard }) });
  assert.equal(p.listeners.size, 0, 'a microphone that is off is not watched');
  await p.click('#suggestions-mic-button');
  assert.equal(p.listeners.size, 1);
  const before = p.reads.length;
  heard = [entity(2), entity(1)];
  p.moveOn();
  await settle();
  assert.equal(p.reads.length, before + 1);
  assert.deepEqual(p.names('#suggestions-stack'), ['PNJ 2', 'PNJ 1']);
  assert.match(p.$('#suggestions-heard').textContent ?? '', /« via 2 »/);
  await p.click('#suggestions-mic-button');
  assert.equal(p.listeners.size, 0);
  assert.equal(p.stops.length, 1);
});

test('a read that fails says so, and the next one that succeeds takes it down', async () => {
  let down = true;
  const p = await pill({}, { failRead: () => down, answer: { suggestions: [entity(1)], history: [entity(1)] } });
  assert.match(p.$('#suggestions-status').textContent ?? '', /Impossible de lire/);
  down = false;
  await p.click('#suggestions-mic-button');
  assert.equal(p.$('#suggestions-status').hidden, true);
  assert.deepEqual(p.names('#suggestions-stack'), ['PNJ 1']);
});

test('an element opens in the centre: the host is asked by the page\'s bus, with the kind of element', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: [entity(3, 'lieu'), entity(2, 'événement'), entity(1, 'pnj')] } });
  await p.click('#suggestion-0');
  await p.click('#suggestion-1');
  await p.click('#suggestion-2');
  assert.deepEqual(p.opened, [{ type: 'place', id: 'e3' }, { type: 'event', id: 'e2' }, { type: 'character', id: 'e1' }]);
  assert.equal(p.sent.length, 0, 'opening an element writes nothing');
  await p.click('#suggestions-history-toggle');
  await p.click('#suggestion-history-2');
  assert.deepEqual(p.opened.at(-1), { type: 'character', id: 'e1' });
});

test('a sound plays on the second click, by soundbox.playSound, aimed at that sound', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: [sound(2), entity(1)] } });
  const row = '#suggestion-0';
  assert.equal(p.$(row).getAttribute('data-writes'), 'soundbox.playSound');
  await p.click(row);
  assert.equal(p.sent.length, 0, 'the first click only arms the sound');
  assert.equal(p.$(row).getAttribute('aria-pressed'), 'true');
  assert.match(p.$(row).textContent ?? '', /cliquer encore/);
  await p.click(row);
  assert.equal(p.sent.length, 1);
  assert.equal(p.sent[0]?.dataCapability, 'soundbox.playSound@1');
  assert.deepEqual(p.sent[0]?.target, { id: 's2' });
  assert.deepEqual(p.sent[0]?.payload, { source: 'suggestion' });
  assert.equal(p.$(row).getAttribute('aria-pressed'), 'false');
  assert.equal(p.opened.length, 0, 'a sound is not opened in the centre');
  await p.click(row);
  assert.equal(p.sent.length, 1, 'a third click arms it again');
});

test('another suggestion takes the arming away from a sound', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: [sound(3), sound(2), entity(1)] } });
  await p.click('#suggestion-0');
  await p.click('#suggestion-1');
  await p.click('#suggestion-0');
  assert.equal(p.sent.length, 0, 'the sound was armed again, not played');
  await p.click('#suggestion-2');
  await p.click('#suggestion-0');
  assert.equal(p.sent.length, 0, 'an element between the two clicks disarms the sound');
});

test('a sound that could not be played says so', async () => {
  const p = await pill({}, { answer: { suggestions: [], history: [sound(2)] }, status: 'rejected', violations: ['not-found'] });
  await p.click('#suggestion-0');
  await p.click('#suggestion-0');
  assert.equal(p.$('#suggestions-status').hidden, false);
  assert.match(p.$('#suggestions-status').textContent ?? '', /« Son 2 » n’a pas pu être joué : introuvable/);
  const down = await pill({}, { answer: { suggestions: [], history: [sound(2)] }, failCommand: true });
  await down.click('#suggestion-0');
  await down.click('#suggestion-0');
  assert.match(down.$('#suggestions-status').textContent ?? '', /n’a pas pu être joué\./);
});

test('what the answer holds is shown as text, never as markup', async () => {
  const p = await pill({}, { answer: [{ ...entity(1), name: '<img src=x onerror=alert(1)>', via: '"><b>' }] });
  assert.equal(p.host.querySelector('#suggestions-stack img'), null);
  assert.equal(p.names('#suggestions-stack')[0], '<img src=x onerror=alert(1)>');
});

test('a new campaign empties the pill and reads it again', async () => {
  const p = await pill({ campaign: 'c1' }, { answer: { suggestions: [entity(1)], history: [entity(1)] } });
  p.mounted.update({ campaign: 'c1' });
  await settle();
  assert.equal(p.reads.length, 1, 'the same campaign reads nothing again');
  p.mounted.update({ campaign: 'c2' });
  await settle();
  assert.equal(p.reads.length, 2);
  assert.deepEqual(p.reads[1]?.variables, { campaign: 'c2' });
});

test('it never reaches for a microphone of its own: only the server listens, locally (live-only-local-audio)', () => {
  const sources = ['index', 'model', 'render', 'ids'].map((name) => readFileSync(new URL(`../src/${name}.ts`, import.meta.url), 'utf8')).join('\n');
  assert.doesNotMatch(sources, /getUserMedia|MediaRecorder|AudioContext|mediaDevices/);
});
