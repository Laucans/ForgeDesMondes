import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Io, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Medium {
  id: string;
  place: string | null;
}
interface Library {
  battlemaps: Medium[];
  illustrations: Medium[];
}
interface Scene {
  id: string;
  places: string[];
  media: string[];
}

const fixture = <T>(name: string): T => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8')) as T;

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  target?: { aggregate: string; id: string };
}

interface Options {
  /** What a command answers, by default `applied` after changing the data as the real DataCapability would. */
  verdict?: (sent: Sent) => { status: string; violations?: string[] };
  /** Capabilities that fail. */
  failing?: string[];
  props?: Record<string, unknown>;
  withContext?: boolean;
}

/** A shell over a small server: the three Capabilities answer the mock-up's fixtures, and a command changes them. */
function fakeShell(options: Options) {
  const library = fixture<Library>('media.listMedia');
  const scenes = fixture<Scene[]>('session.listScenes');
  const places = fixture<unknown[]>('campaign.listPlaces');
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables?: Record<string, unknown> }> = [];
  const context: Array<[string, unknown]> = [];
  const versionListeners: Array<(version: number) => void> = [];

  const apply = (command: Sent): void => {
    if (command.dataCapability === 'media.setMediaPlace@1') {
      const m = [...library.battlemaps, ...library.illustrations].find((x) => x.id === command.payload.media);
      if (m) m.place = (command.payload.place as string | null) ?? null;
    } else if (command.dataCapability === 'session.addPlaceToSession@1') {
      const scene = scenes[0] as Scene;
      const place = command.payload.place as string;
      if (!scene.places.includes(place)) scene.places.push(place);
    } else if (command.dataCapability === 'media.uploadMedia@1') {
      const base = { id: 'new-1', title: command.payload.title, file: command.payload.filePath, size: '1 Mo', place: command.payload.place ?? null };
      if (command.payload.kind === 'battlemap') library.battlemaps.push({ ...base, grid: { orient: 'pointy', size: 32 } } as Medium);
      else library.illustrations.push(base as Medium);
    }
  };
  const verdict = options.verdict ?? ((): { status: string; violations?: string[] } => ({ status: 'applied' }));

  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      if (options.failing?.includes(id)) throw new Error(`${id} is down`);
      const data = id === 'media.listMedia' ? library : id === 'session.listScenes' ? scenes : id === 'campaign.listPlaces' ? places : undefined;
      return { asOf: 1, data: structuredClone(data) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = verdict(command);
      if (outcome.status === 'applied') apply(command);
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult() {
      return { result: { status: 'applied', violations: [] } };
    },
    onDataVersion(listener) {
      versionListeners.push(listener);
      return () => undefined;
    },
    ...(options.withContext === false ? {} : { context: { set: (name: string, value: unknown) => void context.push([name, value]) } }),
  };
  return { shell, sent, reads, context, versionListeners, scenes, library };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the card finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function card(options: Options = {}) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="MediaLibrary"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  let keys = 0;
  const io: Io = { key: () => `key-${(keys += 1)}`, measure: async () => ({ width: 900, height: 600 }) };
  const mounted = mount(host, options.props ?? { session: 's4', sessionNumber: 4 }, fake.shell, timing, io);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the card`);
    return found;
  };
  const textOf = (selector: string): string => $(selector).textContent ?? '';
  const choose = async (selector: string, value: string): Promise<void> => {
    const field = $(selector) as HTMLSelectElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
    await settle();
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const pick = async (name: string, type = 'image/jpeg'): Promise<void> => {
    const input = $('#media-upload-file') as HTMLInputElement;
    Object.defineProperty(input, 'files', { value: [new dom.window.File(['x'], name, { type })], configurable: true });
    input.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
    await settle();
  };
  return { host, dom, mounted, $, textOf, choose, click, pick, ...fake };
}

test('it shows the battlemaps and the illustrations with their place, as the mock-up does', async () => {
  const c = await card();
  assert.equal(c.host.querySelectorAll('#media-battlemaps-list > li').length, 2);
  assert.equal(c.host.querySelectorAll('#media-illustrations-list > li').length, 5);
  assert.equal(c.textOf('#media-count'), '2 battlemaps · 5 illustrations');
  assert.equal((c.$('#media-place-bm-moulin') as HTMLSelectElement).value, 'moulin');
  assert.equal((c.$('#media-place-ill-cover') as HTMLSelectElement).value, 'valombre');
  assert.equal((c.$('#media-place-ill-gaspard') as HTMLSelectElement).value, '');
  assert.match(c.textOf('#media-bm-gue'), /Le Gué des Corbeaux/);
  assert.match(c.textOf('#media-bm-moulin'), /grille côté plat 34 px/);
  assert.match(c.textOf('#media-ill-cover'), /ratio 1,78/);
  assert.deepEqual(c.reads.map((r) => r.id).sort(), ['campaign.listPlaces', 'media.listMedia', 'session.listScenes']);
  assert.deepEqual(c.reads.find((r) => r.id === 'session.listScenes')?.variables, { session: 's4' });
});

test('a medium whose place is in a scene of session 4 reads « Associé à la séance 4 »', async () => {
  const c = await card();
  assert.match(c.textOf('#media-session-ill-cover'), /Associé à la séance 4 via Valombre/);
  assert.match(c.textOf('#media-session-ill-salle-sanglier'), /Associé à la séance 4 via La Taverne du Sanglier/);
  assert.equal(c.host.querySelector('#media-ill-cover button[data-writes="session.addPlaceToSession"]'), null);
});

test('a medium with no place shows no association, no button and no error', async () => {
  const c = await card();
  assert.doesNotMatch(c.textOf('#media-ill-gaspard'), /Associé/);
  assert.equal(c.host.querySelector('#media-ill-gaspard button'), null);
  assert.match(c.textOf('#media-session-ill-gaspard'), /Sans lieu/);
  assert.equal(c.textOf('#media-message'), '');
});

test('a place outside the session offers one button; adding it attaches every card of that place', async () => {
  const c = await card();
  assert.match(c.textOf('#media-session-bm-moulin'), /Lieu hors de la séance 4/);
  assert.match(c.textOf('#media-session-ill-moulin'), /Lieu hors de la séance 4/);
  await c.click('#media-bm-moulin button[data-writes="session.addPlaceToSession"]');
  assert.equal(c.sent.length, 1);
  assert.equal(c.sent[0]?.dataCapability, 'session.addPlaceToSession@1');
  assert.deepEqual(c.sent[0]?.payload, { session: 's4', place: 'moulin' });
  assert.deepEqual(c.sent[0]?.target, { aggregate: 'Session', id: 's4' });
  for (const id of ['bm-moulin', 'ill-moulin']) assert.match(c.textOf(`#media-session-${id}`), /Associé à la séance 4/);
  assert.equal(c.host.querySelector('button[data-writes="session.addPlaceToSession"]'), null);
  assert.match(c.textOf('#media-message'), /est dans la séance/);
});

test('changing a medium’s place follows the new place; clearing it leaves the medium valid', async () => {
  const c = await card();
  await c.choose('#media-place-ill-gaspard', 'valombre');
  assert.equal(c.sent[0]?.dataCapability, 'media.setMediaPlace@1');
  assert.deepEqual(c.sent[0]?.payload, { media: 'ill-gaspard', place: 'valombre' });
  assert.match(c.textOf('#media-session-ill-gaspard'), /Associé à la séance 4/);
  await c.choose('#media-place-ill-gaspard', 'moulin');
  assert.match(c.textOf('#media-session-ill-gaspard'), /Lieu hors de la séance 4/);
  await c.choose('#media-place-ill-gaspard', '');
  assert.deepEqual(c.sent[2]?.payload, { media: 'ill-gaspard', place: null });
  assert.doesNotMatch(c.textOf('#media-ill-gaspard'), /Associé/);
  assert.equal(c.textOf('#media-message').includes('Refusé'), false);
});

test('choosing the place a medium already has sends nothing', async () => {
  const c = await card();
  await c.choose('#media-place-ill-cover', 'valombre');
  assert.equal(c.sent.length, 0);
});

test('a refused command is told with its reason and the card keeps what it knew', async () => {
  const c = await card({ verdict: () => ({ status: 'rejected', violations: ['media-place-is-lieu'] }) });
  await c.choose('#media-place-ill-gaspard', 'valombre');
  assert.match(c.textOf('#media-message'), /Refusé : ce lieu n’est pas un lieu de la campagne/);
  assert.equal((c.$('#media-place-ill-gaspard') as HTMLSelectElement).value, '');
  assert.doesNotMatch(c.textOf('#media-ill-gaspard'), /Associé/);
});

test('a command that settles late is followed until it is applied', async () => {
  let asked = 0;
  const c = await card();
  const original = c.shell.command.bind(c.shell);
  c.shell.command = async (body) => {
    const answer = (await original(body)) as { commandId: string };
    return { commandId: answer.commandId, result: { status: 'queued', violations: [] } };
  };
  c.shell.commandResult = async () => ({ result: { status: (asked += 1) < 3 ? 'queued' : 'applied', violations: [] } });
  await c.choose('#media-place-ill-gaspard', 'valombre');
  assert.equal(asked, 3);
  assert.match(c.textOf('#media-session-ill-gaspard'), /Associé à la séance 4/);
});

test('every battlemap card has « Régler la grille », which opens GridSettings on it; an illustration has none', async () => {
  const c = await card();
  for (const id of ['bm-gue', 'bm-moulin']) assert.match(c.textOf(`#media-${id} button[data-action^="grid:"]`), /Régler la grille/);
  assert.equal(c.host.querySelector('#media-illustrations-list button[data-action^="grid:"]'), null);
  await c.click('#media-bm-moulin button[data-action="grid:bm-moulin"]');
  assert.deepEqual(c.context, [['battlemap', 'bm-gue'], ['battlemap', 'bm-moulin']]);
  assert.ok(c.$('#media-bm-moulin').classList.contains('is-selected'));
  assert.equal(c.sent.length, 0);
});

test('the upload form sends the kind, the title from the file, the optional place, by identifier', async () => {
  const c = await card();
  await c.pick('la-cave_du-sanglier.jpg');
  assert.equal((c.$('#media-upload-title') as HTMLInputElement).value, 'la cave du sanglier');
  assert.match(c.textOf('.kit-filedrop__name'), /la-cave_du-sanglier\.jpg/);
  await c.choose('#media-upload-place', 'moulin');
  await c.click('#media-upload-submit');
  assert.equal(c.sent[0]?.dataCapability, 'media.uploadMedia@1');
  assert.deepEqual(c.sent[0]?.payload, { kind: 'battlemap', title: 'la cave du sanglier', filePath: 'la-cave_du-sanglier.jpg', widthPx: 900, heightPx: 600, place: 'moulin' });
  assert.equal(c.host.querySelectorAll('#media-battlemaps-list > li').length, 3);
  // The new battlemap is the one whose grid is set next; the form is empty again.
  assert.deepEqual(c.context, [['battlemap', 'bm-gue'], ['battlemap', 'new-1']]);
  assert.equal((c.$('#media-upload-title') as HTMLInputElement).value, '');
  assert.equal(c.host.querySelector('.kit-filedrop.has-file'), null);
});

test('an upload with no place is accepted and carries none; an illustration is not a battlemap', async () => {
  const c = await card();
  await c.pick('portrait.png', 'image/png');
  (c.$('input[name="media-kind"][value="illustration"]') as HTMLInputElement).checked = true;
  c.$('input[name="media-kind"][value="illustration"]').dispatchEvent(new c.dom.window.Event('change', { bubbles: true }));
  await c.click('#media-upload-submit');
  assert.deepEqual(c.sent[0]?.payload, { kind: 'illustration', title: 'portrait', filePath: 'portrait.png', widthPx: 900, heightPx: 600 });
  assert.deepEqual(c.context, [['battlemap', 'bm-gue']]);
  assert.equal(c.host.querySelectorAll('#media-illustrations-list > li').length, 6);
});

test('an upload with no file, or a file of another type, sends nothing', async () => {
  const c = await card();
  await c.click('#media-upload-submit');
  assert.match(c.textOf('#media-message'), /Choisissez d’abord une image/);
  await c.pick('notes.pdf', 'application/pdf');
  assert.ok(c.$('#media-upload-drop-zone').classList.contains('has-error'));
  await c.click('#media-upload-submit');
  assert.equal(c.sent.length, 0);
});

test('with no session, nothing is attached and no button adds a place', async () => {
  const c = await card({ props: {} });
  assert.deepEqual(c.reads.map((r) => r.id).sort(), ['campaign.listPlaces', 'media.listMedia']);
  assert.doesNotMatch(c.textOf('#media-library'), /Associé|hors de la séance|Sans lieu/);
  assert.equal(c.host.querySelector('button[data-writes="session.addPlaceToSession"]'), null);
  assert.equal((c.$('#media-place-bm-moulin') as HTMLSelectElement).value, 'moulin');
});

test('when the session’s scenes cannot be read the cards say nothing wrong about the session', async () => {
  const c = await card({ failing: ['session.listScenes'] });
  assert.match(c.textOf('#media-message'), /scènes de la séance n’ont pas pu être lues/);
  assert.doesNotMatch(c.textOf('#media-library'), /Associé à la séance|hors de la séance/);
  assert.equal(c.host.querySelectorAll('li[data-id]').length, 7);
});

test('when the library cannot be read, the card says so', async () => {
  const c = await card({ failing: ['media.listMedia'] });
  assert.match(c.textOf('#media-message'), /Impossible de lire la bibliothèque/);
});

test('a place that the campaign does not list is shown by its id', async () => {
  const c = await card({ failing: ['campaign.listPlaces'] });
  assert.match(c.textOf('#media-message'), /Les lieux n’ont pas pu être lus/);
  assert.match(c.textOf('#media-session-ill-cover'), /Associé à la séance 4 via valombre/);
});

test('another screen moving the data makes the card read again; the first notice only tells the version', async () => {
  const c = await card();
  const before = c.reads.length;
  c.versionListeners[0]?.(1);
  await settle();
  assert.equal(c.reads.length, before);
  c.scenes[0]?.places.push('moulin');
  c.versionListeners[0]?.(2);
  await settle();
  assert.equal(c.reads.length, before + 3);
  assert.match(c.textOf('#media-session-bm-moulin'), /Associé à la séance 4/);
});

test('the screen changing the session prop makes the card follow it', async () => {
  const c = await card({ props: { session: 's4', sessionNumber: 4 } });
  c.mounted.update({ session: 's4', sessionNumber: 5 });
  await settle();
  assert.match(c.textOf('#media-session-ill-cover'), /Associé à la séance 5/);
});

test('every actuator names the DataCapability it triggers and every read part the Capabilities it shows', async () => {
  const c = await card();
  const writes = new Set([...c.host.querySelectorAll<HTMLElement>('[data-writes]')].map((e) => e.dataset.writes));
  assert.deepEqual([...writes].sort(), ['media.setMediaPlace', 'media.uploadMedia', 'session.addPlaceToSession']);
  const reads = new Set([...c.host.querySelectorAll<HTMLElement>('[data-reads]')].flatMap((e) => (e.dataset.reads ?? '').split(' ')));
  assert.deepEqual([...reads].sort(), ['campaign.listPlaces', 'media.listMedia', 'session.listScenes']);
  // No literal colour, font or size: the look is the design system's tokens.
  const style = c.dom.window.document.querySelector('style[data-micro-ui="MediaLibrary"]')?.textContent ?? '';
  assert.doesNotMatch(style, /#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(/);
});

test('a card with no context in its shell still selects the battlemap', async () => {
  const c = await card({ withContext: false });
  await c.click('#media-bm-gue button[data-action="grid:bm-gue"]');
  assert.ok(c.$('#media-bm-gue').classList.contains('is-selected'));
});

test('the battlemap highlighted on opening is the one GridSettings is told about', async () => {
  const c = await card();
  assert.deepEqual(c.context, [['battlemap', 'bm-gue']]);
  assert.ok(c.$('#media-bm-gue').classList.contains('is-selected'));
});

test('a command still queued after the card stops waiting is not reported as a failure', async () => {
  const c = await card({ verdict: () => ({ status: 'parked' }) });
  c.shell.commandResult = async () => ({ result: { status: 'parked', violations: [] } });
  await c.choose('#media-place-ill-gaspard', 'valombre');
  assert.match(c.textOf('#media-message'), /file d’attente \(état : parked\)/);
  assert.doesNotMatch(c.textOf('#media-message'), /pas pu/);
});

test('a second file replaces the title the card filled, but never one the DM typed', async () => {
  const c = await card();
  await c.pick('cave.jpg');
  await c.pick('moulin.png', 'image/png');
  assert.equal((c.$('#media-upload-title') as HTMLInputElement).value, 'moulin');
  const field = c.$('#media-upload-title') as HTMLInputElement;
  field.value = 'Mon titre';
  field.dispatchEvent(new c.dom.window.Event('input', { bubbles: true }));
  await c.pick('autre.png', 'image/png');
  assert.equal(field.value, 'Mon titre');
});

test('a prop changed while a command runs is followed once the command is over, even a refused one', async () => {
  const c = await card({ verdict: () => ({ status: 'rejected', violations: ['media-place-is-lieu'] }) });
  let open: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (open = resolve));
  const original = c.shell.command.bind(c.shell);
  c.shell.command = async (body) => {
    await gate;
    return original(body);
  };
  const changing = c.choose('#media-place-ill-gaspard', 'valombre');
  await settle();
  const before = c.reads.length;
  c.mounted.update({ session: 's5', sessionNumber: 5 });
  await settle();
  assert.equal(c.reads.length, before, 'no read while the command runs');
  open();
  await changing;
  await settle();
  assert.deepEqual(c.reads.filter((r) => r.id === 'session.listScenes').at(-1)?.variables, { session: 's5' });
  assert.match(c.textOf('#media-session-ill-cover'), /Associé à la séance 5/);
});

test('a card that is taken off the page stops following the data version', async () => {
  let stopped = 0;
  const dom = new JSDOM('<!doctype html><body><div id="h"></div></body>');
  const host = dom.window.document.getElementById('h') as HTMLElement;
  const fake = fakeShell({});
  const listeners: Array<(v: number) => void> = [];
  fake.shell.onDataVersion = (listener) => {
    listeners.push(listener);
    return () => void (stopped += 1);
  };
  const mounted = mount(host, { session: 's4', sessionNumber: 4 }, fake.shell, timing);
  await settle();
  host.remove();
  listeners[0]?.(99);
  assert.equal(stopped, 1);
  mounted.destroy();
  assert.equal(stopped, 2);
});
