import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Row {
  id: string;
  name: string;
  tags: string[];
  volume: number;
  file: string;
  duration: number;
  playing?: boolean;
}

/** The mock-up's fixture of the Capability: the library a card is opened on. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/soundbox.listSounds.json', import.meta.url), 'utf8')) as {
  masterVolume: number;
  sounds: Row[];
};

interface Applied {
  status: string;
  violations?: string[];
}

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  targetId?: string;
  caller: { by: string; from: string };
}

/** A shell over a small server: it answers the sounds, and applies a command as the DataGuard would. */
function fakeShell(
  options: {
    read?: () => unknown;
    apply?: (sent: Sent) => Applied;
    later?: number;
    /** Like the real store: sounds are listed for a campaign only, none given answers null. */
    strict?: boolean;
    /** What `session.getLiveSession` answers; a function that throws is a failed read. */
    live?: () => unknown;
    /** What the screen's context holds when the card opens. */
    context?: Record<string, unknown>;
  } = {},
) {
  const data = structuredClone(fixture);
  const sent: Sent[] = [];
  /** The reads of the sounds: `session.getLiveSession` is counted apart. */
  const reads: Array<{ id: string; variables: Record<string, unknown> | undefined }> = [];
  const lookups: Array<Record<string, unknown> | undefined> = [];
  const context = new Map(Object.entries(options.context ?? {}));
  const published: Array<[string, unknown]> = [];
  let asked = 0;
  const apply =
    options.apply ??
    ((command: Sent): Applied => {
      if (command.dataCapability === 'soundbox.addSound@1') {
        const { name, tags, volume, file } = command.payload as { name: string; tags: string[]; volume: number; file: string };
        data.sounds.unshift({ id: 'snd-new', name, tags: tags.map((t) => t.toLowerCase()), volume, file, duration: 2 });
      } else if (command.dataCapability === 'soundbox.updateSound@1') {
        Object.assign(data.sounds.find((s) => s.id === command.targetId) as Row, command.payload);
      } else if (command.dataCapability === 'soundbox.removeSound@1') {
        data.sounds = data.sounds.filter((s) => s.id !== command.targetId);
      }
      return { status: 'applied' };
    });
  let versionListener: ((dataVersion: number) => void) | undefined;
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      if (id === 'session.getLiveSession') {
        lookups.push(variables);
        return { asOf: 1, data: options.live ? options.live() : { session: { id: 's4', campaign: 'cendres-valombre' } } };
      }
      reads.push({ id, variables });
      if (options.strict && variables?.campaign !== 'cendres-valombre') return { asOf: 1, data: null };
      return { asOf: 1, data: options.read ? await options.read() : structuredClone(data) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = apply(command);
      const status = options.later === undefined ? outcome.status : 'queued';
      return { commandId: `c${sent.length}`, result: { status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      asked += 1;
      const outcome = apply(sent[Number(commandId.slice(1)) - 1] as Sent);
      return { result: { commandId, status: asked > (options.later ?? 0) ? outcome.status : 'queued', violations: outcome.violations ?? [] } };
    },
    onDataVersion(listener) {
      versionListener = listener;
      return () => undefined;
    },
    context: {
      get: (name) => context.get(name),
      set(name, value) {
        context.set(name, value);
        published.push([name, value]);
      },
    },
  };
  return { shell, sent, reads, lookups, published, data, moved: (version: number) => versionListener?.(version) };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the card finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function card(options?: Parameters<typeof fakeShell>[0], props: Record<string, unknown> = {}, waiting: Timing = timing) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="SoundLibrary"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, props, fake.shell, waiting);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the card`);
    return found;
  };
  const type = (selector: string, value: string): void => {
    const input = $(selector) as HTMLInputElement;
    input.value = value;
    input.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  /** A file chosen in the FileDrop's input. */
  const choose = (name: string, bytes = 48230): void => {
    const input = $('#sounds-file') as HTMLInputElement;
    Object.defineProperty(input, 'files', { value: [{ name, size: bytes }], configurable: true });
    input.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  };
  const names = (): string[] => [...host.querySelectorAll('#sounds-table-grid tbody tr td:first-child')].map((td) => td.textContent ?? '');
  const text = (selector: string): string => $(selector).textContent ?? '';
  return { dom, host, $, type, click, choose, names, text, ...fake };
}

test('the library opens on the fixture: one row per sound, the count, the tags, the file and the volume', async () => {
  const c = await card();
  assert.equal(c.names().length, fixture.sounds.length);
  assert.ok(c.names().includes('Porte qui grince'));
  assert.match(c.text('#sounds-count'), /^12 sons · \d+ tags$/);
  const porte = c.$('#sound-snd-porte');
  assert.match(porte.textContent ?? '', /porte-qui-grince\.ogg/);
  assert.match(porte.textContent ?? '', /2,4 s/);
  assert.match(porte.textContent ?? '', /80 %/);
  assert.deepEqual([...porte.querySelectorAll('[data-tag]')].map((b) => b.textContent), ['#porte', '#bois', '#intérieur']);
  assert.deepEqual(c.reads.map((r) => r.id), ['soundbox.listSounds']);
});

test('the parts name the units they show and trigger, by identifier', async () => {
  const c = await card();
  assert.ok(c.$('#sounds-table').dataset.reads === 'soundbox.listSounds' && c.$('#sounds-count').dataset.reads === 'soundbox.listSounds');
  assert.equal(c.$('#sounds-new-submit').dataset.writes, 'soundbox.addSound');
  await c.click('[data-edit="snd-porte"]');
  assert.equal(c.$('#sounds-edit-save').dataset.writes, 'soundbox.updateSound');
  assert.equal(c.$('#sounds-edit-delete').dataset.writes, 'soundbox.removeSound');
});

test('typing « porte » keeps only the sounds that match', async () => {
  const c = await card();
  c.type('#sounds-search-input', 'porte');
  assert.deepEqual(c.names(), ['Porte qui grince']);
  c.type('#sounds-search-input', 'Epees');
  assert.deepEqual(c.names(), ['Épées qui s’entrechoquent']);
  c.type('#sounds-search-input', '');
  assert.equal(c.names().length, 12);
});

test('a search with no match says so, and the search field keeps what was typed', async () => {
  const c = await card();
  c.type('#sounds-search-input', 'zzz');
  assert.match(c.text('#sounds-empty'), /Aucun son ne correspond/);
  assert.equal((c.$('#sounds-search-input') as HTMLInputElement).value, 'zzz');
});

test('a click on a tag filters by it, a second click or « Retirer le filtre » lets go', async () => {
  const c = await card();
  await c.click('#sound-snd-foule [data-tag="taverne"]');
  assert.deepEqual(c.names(), ['Bruit de foule en taverne', 'Chope qui se brise']);
  assert.match(c.text('#sounds-filter-badge'), /#taverne/);
  assert.equal(c.$('#sound-snd-chope [data-tag="taverne"]').getAttribute('aria-pressed'), 'true');
  await c.click('#sound-snd-chope [data-tag="taverne"]');
  assert.equal(c.names().length, 12);
  assert.equal(c.host.querySelector('#sounds-filter-badge'), null);
  await c.click('#sound-snd-epees [data-tag="combat"]');
  assert.deepEqual(c.names(), ['Épées qui s’entrechoquent', 'Flèche qui siffle', 'Boule de feu']);
  await c.click('#sounds-filter-clear');
  assert.equal(c.names().length, 12);
});

test('the tag filter and the search work together', async () => {
  const c = await card();
  await c.click('#sound-snd-porte [data-tag="intérieur"]');
  c.type('#sounds-search-input', 'foule');
  assert.deepEqual(c.names(), ['Bruit de foule en taverne']);
});

test('a sound playing now is marked on its row', async () => {
  const c = await card({ read: () => ({ sounds: [{ id: 'a', name: 'A', tags: [], volume: 50, file: 'a.ogg', duration: 1, playing: true }, { id: 'b', name: 'B', tags: [], volume: 50, file: 'b.ogg', duration: 1, playing: false }] }) });
  assert.ok(c.$('#sound-a').classList.contains('is-playing'));
  assert.ok(!c.$('#sound-b').classList.contains('is-playing'));
});

test('a dropped file fills the FileDrop and suggests the name', async () => {
  const c = await card();
  c.choose('volet-qui-claque.ogg');
  assert.ok(c.$('#sounds-drop-field').classList.contains('has-file'));
  assert.equal(c.text('.kit-filedrop__name'), 'volet-qui-claque.ogg');
  assert.equal(c.text('.kit-filedrop__size'), '47,1 Ko');
  assert.equal((c.$('#sounds-new-name') as HTMLInputElement).value, 'Volet qui claque');
  c.choose('porte-ouverte.wav');
  assert.equal((c.$('#sounds-new-name') as HTMLInputElement).value, 'Porte ouverte', 'a suggestion gives way to the next');
  c.type('#sounds-new-name', 'Ma porte');
  c.choose('autre.mp3');
  assert.equal((c.$('#sounds-new-name') as HTMLInputElement).value, 'Ma porte', 'what the DM typed stays');
});

test('a file that is not a sound is refused in the FileDrop and is not sent', async () => {
  const c = await card();
  c.choose('notes.pdf');
  assert.ok(c.$('#sounds-drop-field').classList.contains('has-error'));
  c.type('#sounds-new-name', 'Notes');
  await c.click('#sounds-new-submit');
  assert.match(c.text('#sounds-upload-result'), /fichier/);
  assert.equal(c.sent.length, 0);
});

test('a file dropped with a name and tags is added by soundbox.addSound, and the list refreshes', async () => {
  const c = await card();
  c.choose('volet-qui-claque.ogg');
  c.type('#sounds-new-name', 'Volet qui claque');
  c.type('#sounds-new-tags', 'Fenêtre, bois, extérieur');
  await c.click('#sounds-new-submit');
  assert.equal(c.sent.length, 1);
  const [command] = c.sent;
  assert.equal(command?.dataCapability, 'soundbox.addSound@1');
  assert.deepEqual(command?.payload, { campaign: 'cendres-valombre', name: 'Volet qui claque', tags: ['Fenêtre', 'bois', 'extérieur'], volume: 75, file: 'volet-qui-claque.ogg' });
  assert.equal(command?.caller.from, 'SoundLibrary');
  assert.equal(c.reads.length, 2, 'the sounds are read again');
  assert.equal(c.names()[0], 'Volet qui claque');
  assert.match(c.text('#sounds-count'), /^13 sons/);
  assert.match(c.text('#sounds-upload-result'), /est dans la soundbox/);
  assert.equal((c.$('#sounds-new-name') as HTMLInputElement).value, '', 'the form is empty for the next sound');
  assert.ok(!c.$('#sounds-drop-field').classList.contains('has-file'));
});

test('the campaign the screen names goes with the read and with the new sound', async () => {
  const c = await card(undefined, { campaign: 'cendres-valombre' });
  assert.deepEqual(c.reads[0]?.variables, { campaign: 'cendres-valombre' });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'A');
  await c.click('#sounds-new-submit');
  assert.equal(c.sent[0]?.payload.campaign, 'cendres-valombre');
});

test('a sound with no name or no file sends nothing', async () => {
  const c = await card();
  await c.click('#sounds-new-submit');
  assert.match(c.text('#sounds-upload-result'), /fichier/);
  c.choose('a.ogg');
  c.type('#sounds-new-name', ' ');
  await c.click('#sounds-new-submit');
  assert.match(c.text('#sounds-upload-result'), /nom/);
  assert.equal(c.sent.length, 0);
});

test('a refused addition tells why, keeps the form, and leaves the list alone', async () => {
  const c = await card({ apply: () => ({ status: 'rejected', violations: ['sound-volume-range'] }) });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'A');
  await c.click('#sounds-new-submit');
  assert.match(c.text('#sounds-upload-result'), /Ajout refusé : le volume reste entre 0 et 100 %/);
  assert.equal((c.$('#sounds-new-name') as HTMLInputElement).value, 'A');
  assert.equal(c.reads.length, 1);
});

test('a command that settles later is followed until it does', async () => {
  const c = await card({ later: 2 });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'Lent');
  await c.click('#sounds-new-submit');
  assert.equal(c.names()[0], 'Lent');
});

test('a command that never settles is told, not assumed applied', async () => {
  const c = await card({ apply: () => ({ status: 'queued' }) });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'A');
  await c.click('#sounds-new-submit');
  assert.match(c.text('#sounds-upload-result'), /L’ajout n’a pas abouti \(état : queued\)/);
});

test('a click on « Modifier » opens the form on that sound', async () => {
  const c = await card();
  assert.equal(c.$('#sounds-edit').hidden, true);
  await c.click('[data-edit="snd-foule"]');
  assert.equal(c.$('#sounds-edit').hidden, false);
  assert.equal((c.$('#sounds-edit-name') as HTMLInputElement).value, 'Bruit de foule en taverne');
  assert.equal((c.$('#sounds-edit-tags') as HTMLInputElement).value, 'foule, taverne, intérieur');
  assert.equal((c.$('#sounds-edit-volume') as HTMLInputElement).value, '55');
});

test('moving the volume slider shows its value', async () => {
  const c = await card();
  await c.click('[data-edit="snd-foule"]');
  c.type('#sounds-edit-volume', '30');
  assert.equal(c.text('.kit-slider__out'), '30');
});

test('saving an edit triggers soundbox.updateSound on that sound, and the list refreshes', async () => {
  const c = await card();
  await c.click('[data-edit="snd-porte"]');
  c.type('#sounds-edit-name', 'Porte du donjon');
  c.type('#sounds-edit-tags', 'Porte, fer');
  c.type('#sounds-edit-volume', '60');
  await c.click('#sounds-edit-save');
  const [command] = c.sent;
  assert.equal(command?.dataCapability, 'soundbox.updateSound@1');
  assert.equal(command?.targetId, 'snd-porte');
  assert.deepEqual(command?.payload, { name: 'Porte du donjon', tags: ['Porte', 'fer'], volume: 60 });
  assert.ok(c.names().includes('Porte du donjon'));
  assert.match(c.text('#sound-snd-porte'), /60 %/);
  assert.match(c.text('#sounds-edit-result'), /Son enregistré/);
});

test('an edit with no name is not sent', async () => {
  const c = await card();
  await c.click('[data-edit="snd-porte"]');
  c.type('#sounds-edit-name', '');
  await c.click('#sounds-edit-save');
  assert.match(c.text('#sounds-edit-result'), /nom/);
  assert.equal(c.sent.length, 0);
});

test('« Supprimer » triggers soundbox.removeSound on that sound at once, and the list refreshes', async () => {
  const c = await card();
  await c.click('[data-edit="snd-porte"]');
  await c.click('#sounds-edit-delete');
  const [command] = c.sent;
  assert.equal(command?.dataCapability, 'soundbox.removeSound@1');
  assert.equal(command?.targetId, 'snd-porte');
  assert.ok(!c.names().includes('Porte qui grince'));
  assert.equal(c.names().length, 11);
  assert.equal(c.$('#sounds-edit').hidden, true);
  assert.match(c.text('#sounds-status'), /« Porte qui grince » est supprimé/);
});

test('a sound opened for editing while another is being removed keeps its form, held still until the removal ends', async () => {
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const c = await card({ later: 1 }, {}, { sleep: () => held, everyMs: 0, polls: 5 });
  await c.click('[data-edit="snd-porte"]');
  await c.click('#sounds-edit-delete');
  await c.click('[data-edit="snd-foule"]');
  assert.equal((c.$('#sounds-edit-save') as HTMLButtonElement).disabled, true, 'a click cannot be swallowed by the removal in flight');
  release();
  await settle();
  assert.equal((c.$('#sounds-edit-save') as HTMLButtonElement).disabled, false);
  assert.equal(c.$('#sounds-edit').hidden, false);
  assert.equal((c.$('#sounds-edit-name') as HTMLInputElement).value, 'Bruit de foule en taverne');
  assert.ok(!c.names().includes('Porte qui grince'));
});

test('a refused removal keeps the sound and tells why', async () => {
  const c = await card({ apply: () => ({ status: 'rejected', violations: ['not-found'] }) });
  await c.click('[data-edit="snd-porte"]');
  await c.click('#sounds-edit-delete');
  assert.match(c.text('#sounds-status'), /Suppression refusée : ce son n’existe plus/);
  assert.equal(c.names().length, 12);
});

test('a library that cannot be read says so, and the form to add is still there', async () => {
  const c = await card({ read: () => { throw new Error('down'); } });
  assert.match(c.text('#sounds-table'), /Impossible de lire les sons/);
  assert.ok(c.$('#sounds-new-submit'));
});

test('an empty library invites the first sound', async () => {
  const c = await card({ read: () => ({ sounds: [] }) });
  assert.match(c.text('#sounds-empty'), /Aucun son dans la bibliothèque/);
  assert.equal(c.text('#sounds-count'), '0 son · 0 tag');
});

test('the list is read again when the data moves, but not for the version it opened on', async () => {
  const c = await card();
  c.moved(1);
  await settle();
  assert.equal(c.reads.length, 1);
  c.data.sounds.pop();
  c.moved(2);
  await settle();
  assert.equal(c.reads.length, 2);
  assert.equal(c.names().length, 11);
});

test('an older read that arrives after a newer one does not overwrite it', async () => {
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const row = (id: string, name: string): Row => ({ id, name, tags: [], volume: 50, file: `${id}.ogg`, duration: 1 });
  let asked = 0;
  const c = await card({
    read: async () => {
      asked += 1;
      if (asked === 1) {
        await held;
        return { sounds: [row('old', 'Ancien')] };
      }
      return { sounds: [row('new', 'Récent')] };
    },
  });
  assert.deepEqual(c.names(), [], 'the opening read is still waiting');
  c.moved(1);
  c.moved(2);
  await settle();
  assert.deepEqual(c.names(), ['Récent']);
  release();
  await settle();
  assert.deepEqual(c.names(), ['Récent']);
});

test('a form open on a sound that is gone closes when the list is read again', async () => {
  const c = await card();
  await c.click('[data-edit="snd-porte"]');
  c.data.sounds = c.data.sounds.filter((s) => s.id !== 'snd-porte');
  c.moved(1);
  c.moved(2);
  await settle();
  assert.equal(c.$('#sounds-edit').hidden, true);
  assert.ok(!c.names().includes('Porte qui grince'));
});

test('a command whose follow-up fails is told as failed, and the list is read again to show what is stored', async () => {
  const c = await card({ later: 1 });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'Fantôme');
  let broken = false;
  const original = c.shell.commandResult.bind(c.shell);
  c.shell.commandResult = async (id) => {
    broken = true;
    await original(id);
    throw new Error('network');
  };
  await c.click('#sounds-new-submit');
  assert.ok(broken);
  assert.match(c.text('#sounds-upload-result'), /L’ajout a échoué/);
  assert.equal(c.reads.length, 2, 'the list is read again');
});

test('a tag is pressed whatever the case or accents of the one chosen', async () => {
  const c = await card({ read: () => ({ sounds: [{ id: 'a', name: 'A', tags: ['Épée'], volume: 1, file: 'a.ogg', duration: 1 }, { id: 'b', name: 'B', tags: ['epee'], volume: 1, file: 'b.ogg', duration: 1 }] }) });
  await c.click('#sound-a [data-tag]');
  assert.equal(c.$('#sound-a [data-tag]').getAttribute('aria-pressed'), 'true');
  assert.equal(c.$('#sound-b [data-tag]').getAttribute('aria-pressed'), 'true');
  await c.click('#sound-b [data-tag]');
  assert.equal(c.names().length, 2);
  assert.equal(c.host.querySelector('#sounds-filter-badge'), null, 'the same tag chosen again lets go');
});

// AC1: given the live session of cendres-valombre, when the screen gives no campaign, every sound of the fixture is a row with its « Modifier » button
test('with no campaign given, the library lists the running session’s campaign', async () => {
  const c = await card({ strict: true });
  assert.deepEqual(c.lookups, [undefined], 'the running session is asked for, no id');
  assert.deepEqual(c.reads.map((r) => r.variables), [{ campaign: 'cendres-valombre' }]);
  assert.equal(c.names().length, fixture.sounds.length);
  for (const sound of fixture.sounds) assert.ok(c.$(`#sound-${sound.id} [data-edit="${sound.id}"]`).textContent?.includes('Modifier'), `${sound.id} has its Modifier button`);
  assert.equal(c.$('#sounds-campaign').dataset.reads, 'session.getLiveSession');
  assert.equal(c.text('#sounds-campaign'), 'Campagne : cendres-valombre');
});

// AC2: given the same screen, when a sound is removed with « Modifier » then « Supprimer », its row disappears and the others remain
test('on the session’s campaign, a removed sound leaves its row and the others stay', async () => {
  const c = await card({ strict: true });
  await c.click('[data-edit="snd-porte"]');
  await c.click('#sounds-edit-delete');
  assert.deepEqual(c.sent.map((s) => [s.dataCapability, s.targetId]), [['soundbox.removeSound@1', 'snd-porte']]);
  assert.equal(c.host.querySelector('#sound-snd-porte'), null);
  assert.equal(c.names().length, fixture.sounds.length - 1);
  assert.deepEqual(c.reads.map((r) => r.variables), [{ campaign: 'cendres-valombre' }, { campaign: 'cendres-valombre' }], 'read again on the same campaign');
  assert.equal(c.lookups.length, 1, 'the session is asked for once');
});

// AC3: given the same screen, when the campaign is found, it is published to the screen's context for LiveSoundbox
test('the campaign found is published to the screen’s context', async () => {
  const c = await card({ strict: true });
  assert.deepEqual(c.published, [['campaign', 'cendres-valombre']]);
});

test('a new sound is added to the campaign found', async () => {
  const c = await card({ strict: true });
  c.choose('a.ogg');
  c.type('#sounds-new-name', 'A');
  await c.click('#sounds-new-submit');
  assert.equal(c.sent[0]?.payload.campaign, 'cendres-valombre');
});

test('a campaign the screen names is used as it is: the session is not asked, nothing is published', async () => {
  const c = await card({ strict: true }, { campaign: 'cendres-valombre' });
  assert.equal(c.lookups.length, 0);
  assert.deepEqual(c.published, []);
  assert.equal(c.names().length, fixture.sounds.length);
});

test('a campaign the screen’s context already holds is used without asking the session', async () => {
  const c = await card({ strict: true, context: { campaign: 'cendres-valombre' } });
  assert.equal(c.lookups.length, 0);
  assert.deepEqual(c.published, []);
  assert.equal(c.names().length, fixture.sounds.length);
});

test('with no session under way, the library reads as it did and publishes nothing', async () => {
  const c = await card({ strict: true, live: () => null });
  assert.deepEqual(c.reads.map((r) => r.variables), [{}]);
  assert.deepEqual(c.published, []);
  assert.match(c.text('#sounds-table'), /Aucun son/);
  assert.equal(c.text('#sounds-campaign'), '');
});

test('a session read that fails leaves the library to its plain read, and the next read asks again', async () => {
  let down = true;
  const c = await card({
    strict: true,
    live: () => {
      if (down) throw new Error('down');
      return { session: { id: 's4', campaign: 'cendres-valombre' } };
    },
  });
  assert.deepEqual(c.reads.map((r) => r.variables), [{}]);
  assert.equal(c.lookups.length, 1);
  down = false;
  c.moved(1); // the first notice only tells the version the screen opened on
  c.moved(2);
  await settle();
  assert.equal(c.lookups.length, 2, 'asked again');
  assert.deepEqual(c.reads.at(-1)?.variables, { campaign: 'cendres-valombre' });
  assert.equal(c.names().length, fixture.sounds.length);
  assert.deepEqual(c.published, [['campaign', 'cendres-valombre']]);
});

test('a context that fails still leaves the sounds listed', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const fake = fakeShell({ strict: true, context: { campaign: 'x' } });
  const broken: Shell = {
    ...fake.shell,
    context: {
      get: () => {
        throw new Error('no context');
      },
      set: () => undefined,
    },
  };
  mount(host, {}, broken, timing);
  await settle();
  assert.deepEqual(fake.reads.map((r) => r.variables), [{}], 'the plain read is made');
  assert.equal(host.querySelector('#sounds-table')?.textContent?.includes('Chargement'), false, 'the list is not left loading');
});

test('a context that cannot be written still lists the campaign’s sounds, and the next read publishes again', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const fake = fakeShell({ strict: true });
  let writable = false;
  const attempts: unknown[] = [];
  const shaky: Shell = {
    ...fake.shell,
    context: {
      get: () => undefined,
      set: (name, value) => {
        attempts.push([name, value]);
        if (!writable) throw new Error('no context');
        fake.shell.context.set(name, value);
      },
    },
  };
  mount(host, {}, shaky, timing);
  await settle();
  assert.deepEqual(fake.reads.map((r) => r.variables), [{ campaign: 'cendres-valombre' }], 'the sounds are read on the campaign found');
  assert.equal(host.querySelector('#sounds-table')?.textContent?.includes('Chargement'), false, 'the list is not left loading');
  assert.deepEqual(fake.published, []);
  writable = true;
  fake.moved(1); // the first notice only tells the version the screen opened on
  fake.moved(2);
  await settle();
  assert.equal(attempts.length, 2, 'publishing is tried again');
  assert.deepEqual(fake.published, [['campaign', 'cendres-valombre']]);
  assert.equal(fake.lookups.length, 1, 'the session is not asked twice');
  assert.deepEqual(fake.reads.at(-1)?.variables, { campaign: 'cendres-valombre' });
});
