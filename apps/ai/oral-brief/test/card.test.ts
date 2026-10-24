import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Dictation, type Timing } from '../src/index.ts';
import { STYLE } from '../src/style.ts';
import type { Shell } from '../src/types.ts';

interface Answer {
  session: string;
  lastRun: string | null;
  duration: string | null;
  transcript: string[];
  result: {
    scenes: Array<{ order: number; title: string; links: string[] }>;
    recognized: Array<{ id: string; type: string; name: string; heard: string | null; updated: boolean; change?: string }>;
    missing: Array<{ id: string; type: string; name: string }>;
  };
}

/** The mock-up's fixture: session 4's last brief, as `session.getLastBrief` answers it. */
const last = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/session.getLastBrief.json', import.meta.url), 'utf8')) as Answer;
const empty = (session: string): Answer => ({ session, lastRun: null, duration: null, transcript: [], result: { scenes: [], recognized: [], missing: [] } });

/** What the analysis of a brief leaves: two scenes, one element recognised, one missing. */
const analysed = (session: string, text: string): Answer => ({
  session,
  lastRun: '2026-10-18T09:00:00',
  duration: '00:42',
  transcript: text.split('\n'),
  result: {
    scenes: [
      { order: 1, title: 'La nouvelle scène', links: ['Valombre'] },
      { order: 2, title: 'La suite', links: ['Pont des Pendus (à créer)'] },
    ],
    recognized: [{ id: 'valombre', type: 'lieu', name: 'Valombre', heard: 'Valombre', updated: false }],
    missing: [{ id: 'tc-pont', type: 'lieu', name: 'Le Pont des Pendus' }],
  },
});

interface Sent {
  dataCapability: string;
  payload: { session: string; text: string };
  caller: { by: string; from: string };
}

interface Applied {
  status: string;
  violations?: string[];
}

/** A shell over a small server: it answers the session's last brief, and applies a command as the DataGuard would. */
function fakeShell(options: { apply?: (sent: Sent) => Applied; unreadable?: boolean; polls?: boolean; settleAs?: string } = {}) {
  const briefs = new Map<string, Answer>([['s4', structuredClone(last)]]);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables: Record<string, unknown> | undefined }> = [];
  const results: string[] = [];
  const apply =
    options.apply ??
    ((command: Sent): Applied => {
      briefs.set(command.payload.session, analysed(command.payload.session, command.payload.text));
      return { status: 'applied' };
    });
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      if (options.unreadable && sent.length > 0) throw new Error('down');
      return { asOf: 1, data: structuredClone(briefs.get(String(variables?.session)) ?? empty(String(variables?.session))) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = apply(command);
      if (options.polls) return { commandId: `c${sent.length}`, result: { status: 'queued', violations: [] } };
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      results.push(commandId);
      return { result: { commandId, status: options.settleAs ?? 'applied', violations: [] } };
    },
  };
  return { shell, sent, reads, results, briefs };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the screen finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function screen(options?: Parameters<typeof fakeShell>[0], props: Record<string, unknown> = { session: 's4' }, dictation?: Dictation) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="OralBrief"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing, dictation);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the screen`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const type = (value: string): void => {
    const area = $('#brief-text') as HTMLTextAreaElement;
    area.value = value;
    area.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  return { document, dom, host, mounted, $, click, type, text: () => host.textContent ?? '', ...fake };
}

test('the screen opens on the last brief, read by identifier alone: its transcript, duration, last run and the scenes in order with their links', async () => {
  const s = await screen();
  assert.deepEqual(s.reads, [{ id: 'session.getLastBrief', sha256: undefined, variables: { session: 's4' } }]);
  const lines = Array.from(s.$('#brief-transcript').children).map((li) => li.textContent);
  assert.deepEqual(lines, last.transcript);
  assert.match(s.$('#brief-last').textContent ?? '', /^Dernier brief : .*octobre.* · 04:12$/);
  assert.match(s.$('#brief-transcript-card').textContent ?? '', /04:12/);
  const scenes = Array.from(s.$('#brief-scenes ol').children);
  assert.equal(scenes.length, 3);
  last.result.scenes.forEach((scene, i) => {
    assert.equal(scenes[i]?.querySelector('b')?.textContent, scene.title);
    assert.equal(scenes[i]?.querySelector('small')?.textContent, scene.links.join(' · '));
  });
  assert.match(s.$('#brief-scenes h3').textContent ?? '', /^3 scènes créées$/);
});

test('recognised elements show as existing, the updated ones marked with their change; the missing ones as « à créer », with nothing to create them', async () => {
  const s = await screen();
  const known = last.result.recognized;
  assert.equal(s.$('#brief-recognized ul').children.length, known.length);
  for (const x of known) {
    const item = s.$(`#brief-known-${x.id}`);
    assert.match(item.textContent ?? '', /Existe/);
    assert.equal(item.querySelector('a')?.getAttribute('href'), `fiche-entite.html?id=${x.id}`);
    assert.equal(/Mis à jour/.test(item.textContent ?? ''), x.updated);
    assert.equal(item.querySelector('[data-diff]') !== null, x.updated);
  }
  assert.match(s.$('#brief-known-morlox').textContent ?? '', /Notes, comportement et alias mis à jour/);
  assert.match(s.$('#brief-known-gaspard').textContent ?? '', /entendu : « Gaspard »/);
  const missing = s.$('#brief-missing');
  assert.equal(missing.querySelectorAll('li').length, last.result.missing.length);
  for (const x of last.result.missing) assert.match(s.$(`#brief-missing-${x.id}`).textContent ?? '', new RegExp(`${x.name}.*à créer`));
  assert.match(s.$('#brief-missing h3').textContent ?? '', /3 manquantes → « à créer »/);
  // brief-no-auto-creation: the only actuator that writes is the analysis.
  const writers = Array.from(s.host.querySelectorAll<HTMLElement>('[data-writes]')).map((el) => el.dataset.writes);
  assert.deepEqual(writers, ['ai.analyzeBrief']);
});

test('every part names the unit it shows or triggers, in the kit’s own classes, over tokens only', async () => {
  const s = await screen();
  assert.equal(s.$('#brief-analyse').dataset.writes, 'ai.analyzeBrief');
  assert.equal(s.$('#brief-result').dataset.reads, 'session.getLastBrief');
  assert.equal(s.$('#brief-last').dataset.reads, 'session.getLastBrief');
  for (const kit of ['.kit-micbutton', '.kit-textarea', '.kit-button', '.kit-card', '.kit-badge']) s.$(kit);
  assert.doesNotMatch(STYLE, /#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(|\d+px/);
  assert.equal(s.document.querySelectorAll('style[data-micro-ui="OralBrief"]').length, 1);
});

test('« Voir le diff » tells the page which sheet to open', async () => {
  const s = await screen();
  const heard: unknown[] = [];
  s.document.body.addEventListener('open-versions', (e) => heard.push((e as CustomEvent).detail));
  await s.click('#brief-diff-morlox');
  assert.deepEqual(heard, [{ id: 'morlox' }]);
});

test('a session with no brief says so, and offers the analysis all the same', async () => {
  const s = await screen(undefined, { session: 's9' });
  assert.match(s.$('#brief-none').textContent ?? '', /Aucun brief/);
  assert.equal(s.$('#brief-last').textContent, '');
  assert.equal((s.$('#brief-analyse') as HTMLButtonElement).disabled, true);
});

test('typing a brief and launching the analysis sends the text alone, then reads the brief again and shows the new scenes', async () => {
  const s = await screen();
  const analyse = s.$('#brief-analyse') as HTMLButtonElement;
  assert.equal(analyse.disabled, true);
  s.type('  On commence au pont des Pendus.  ');
  assert.equal(analyse.disabled, false);
  await s.click('#brief-analyse');
  assert.deepEqual(s.sent, [
    { dataCapability: 'ai.analyzeBrief@1', payload: { session: 's4', text: 'On commence au pont des Pendus.' }, caller: { by: 'dm', from: 'OralBrief', roles: [] } },
  ]);
  assert.equal(s.reads.length, 2);
  assert.deepEqual(Array.from(s.$('#brief-scenes ol').children).map((li) => li.querySelector('b')?.textContent), ['La nouvelle scène', 'La suite']);
  assert.match(s.$('#brief-missing').textContent ?? '', /Le Pont des Pendus/);
  assert.match(s.$('#brief-done').textContent ?? '', /2 scènes, 1 à créer/);
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).value, '');
  assert.equal(analyse.disabled, true);
});

test('an empty brief sends nothing and takes the focus', async () => {
  const s = await screen();
  s.type('   ');
  assert.equal((s.$('#brief-analyse') as HTMLButtonElement).disabled, true);
  (s.$('#brief-analyse') as HTMLButtonElement).disabled = false;
  await s.click('#brief-analyse');
  assert.deepEqual(s.sent, []);
  assert.equal(s.document.activeElement, s.$('#brief-text'));
});

test('the microphone: one click listens, « Terminer et analyser » ends it and sends what was heard, as text', async () => {
  const s = await screen(undefined, { session: 's4' }, { transcript: () => 'Première scène à la taverne.\nDeuxième scène au pont.' });
  assert.equal((s.$('#brief-analyse') as HTMLButtonElement).disabled, true);
  await s.click('#brief-mic');
  assert.equal(s.$('#brief-mic-box').getAttribute('data-state'), 'recording');
  assert.equal(s.$('#brief-mic').getAttribute('aria-pressed'), 'true');
  assert.match(s.$('#brief-mic').getAttribute('aria-label') ?? '', /Brief oral/);
  assert.equal((s.$('#brief-analyse') as HTMLButtonElement).disabled, false);
  await s.click('#brief-analyse');
  assert.equal(s.sent.length, 1);
  assert.deepEqual(Object.keys(s.sent[0]?.payload ?? {}).sort(), ['session', 'text']);
  assert.equal(s.sent[0]?.payload.text, 'Première scène à la taverne.\nDeuxième scène au pont.');
  assert.equal(s.$('#brief-mic-box').getAttribute('data-state'), 'idle');
  assert.match(s.$('#brief-done').textContent ?? '', /Brief analysé/);
});

test('a second click pauses the microphone and hands the transcript to the text area, after what is typed', async () => {
  const s = await screen(undefined, { session: 's4' }, { transcript: () => 'dicté' });
  s.type('tapé');
  await s.click('#brief-mic');
  await s.click('#brief-mic');
  assert.equal(s.$('#brief-mic-box').getAttribute('data-state'), 'paused');
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).value, 'tapé\ndicté');
  await s.click('#brief-mic');
  assert.equal(s.$('#brief-mic-box').getAttribute('data-state'), 'recording');
  await s.click('#brief-mic');
  assert.equal(s.$('#brief-mic-box').getAttribute('data-state'), 'paused');
  assert.deepEqual(s.sent, []);
});

test('with nobody to hear the microphone, a dictation that gives nothing leaves the text area as it was', async () => {
  const s = await screen();
  s.type('tapé');
  await s.click('#brief-mic');
  await s.click('#brief-mic');
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).value, 'tapé');
});

test('a command still queued is followed until the server has applied it', async () => {
  const s = await screen({ polls: true });
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.deepEqual(s.results, ['c1']);
  assert.match(s.$('#brief-done').textContent ?? '', /Brief analysé/);
});

test('a refused analysis says why and keeps the text and the last brief', async () => {
  const s = await screen({ apply: () => ({ status: 'rejected', violations: ['text-only-no-audio'] }) });
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.match(s.$('#brief-error').textContent ?? '', /Analyse refusée : seul du texte est accepté\./);
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).value, 'un brief');
  assert.equal(s.reads.length, 1);
  assert.equal(s.$('#brief-scenes ol').children.length, 3);
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).disabled, false);
});

test('an analysis that does not settle says where it stopped', async () => {
  const s = await screen({ polls: true, settleAs: 'parked' });
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.match(s.$('#brief-error').textContent ?? '', /n’a pas abouti \(état : parked\)/);
  assert.equal(s.results.length, 5);
});

test('an analysis applied but whose result cannot be read says so, and the screen is usable again', async () => {
  const s = await screen({ unreadable: true });
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.match(s.$('#brief-error').textContent ?? '', /analysé, mais son résultat n’a pas pu être relu/);
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).disabled, false);
});

test('a shell that fails says the analysis failed', async () => {
  const s = await screen({
    apply: () => {
      throw new Error('down');
    },
  });
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.match(s.$('#brief-error').textContent ?? '', /L’analyse a échoué/);
});

test('while the analysis runs, the screen holds still and a second launch is ignored', async () => {
  const s = await screen();
  s.type('un brief');
  const first = s.click('#brief-analyse');
  assert.equal((s.$('#brief-text') as HTMLTextAreaElement).disabled, true);
  assert.equal((s.$('#brief-mic') as HTMLButtonElement).disabled, true);
  assert.equal((s.$('#brief-analyse') as HTMLButtonElement).disabled, true);
  await first;
  assert.equal(s.sent.length, 1);
});

test('no session chosen: nothing is read, and the analysis says why', async () => {
  const s = await screen(undefined, {});
  assert.deepEqual(s.reads, []);
  s.type('un brief');
  await s.click('#brief-analyse');
  assert.deepEqual(s.sent, []);
  assert.match(s.$('#brief-error').textContent ?? '', /Aucune séance choisie/);
});

test('following the page’s session: another session is read, the same one is not', async () => {
  const s = await screen();
  s.mounted.update({ session: 's4' });
  await settle();
  assert.equal(s.reads.length, 1);
  s.mounted.update({ session: 's9' });
  await settle();
  assert.deepEqual(s.reads.map((r) => r.variables), [{ session: 's4' }, { session: 's9' }]);
  assert.match(s.$('#brief-none').textContent ?? '', /Aucun brief/);
});

test('a brief that cannot be read says so', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const shell: Shell = { capability: () => Promise.reject(new Error('down')), command: () => Promise.reject(new Error('no')), commandResult: () => Promise.reject(new Error('no')) };
  mount(host, { session: 's4' }, shell, timing);
  await settle();
  assert.match(host.querySelector('#brief-error')?.textContent ?? '', /Impossible de lire le dernier brief/);
});
