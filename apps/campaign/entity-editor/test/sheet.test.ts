import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Mounted, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

type Sheet = Record<string, unknown> & { id: string; kind: string; stats?: Record<string, number> };

const fixture = <T>(name: string): T => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8')) as T;
const characters = fixture<Array<Sheet & { initials?: string }>>('campaign.listCharacters');
const places = fixture<Sheet[]>('campaign.listPlaces');
const players = fixture<unknown>('campaign.listPlayers');
const media = fixture<unknown>('media.listMedia');

/** An element of the mock-up's lists, as `campaign.getEntity` answers it. */
function answer(element: Sheet | undefined): unknown {
  if (!element) return null;
  const base = { id: element.id, name: element.name, aliases: element.aliases, kind: element.kind ?? 'lieu', description: element.description, notes: element.notes ?? '' };
  if (!element.stats) return base;
  return {
    ...base,
    player: element.kind === 'pj' ? element.player : null,
    prompt: element.prompt,
    background: element.background,
    behavior: element.behavior,
    stats: element.stats,
    ca: element.ca,
    pvMax: element.pvMax,
    features: element.features,
    color: element.color ?? 'accent',
    initials: element.initials,
    portrait: element.portrait === true,
    tokenImage: false,
    crop: element.crop ?? null,
    ddb: element.ddb ?? null,
  };
}

interface Sent {
  dataCapability: string;
  target: { aggregate: string; id?: string };
  payload: Record<string, unknown> & { entity?: Record<string, unknown> };
  idempotencyKey: string;
  caller: { by: string; from: string; roles: string[] };
}

interface Options {
  /** What the server does with a command; the default applies the sheet's own effects. */
  apply?: (sent: Sent, store: Sheet[]) => { status: string; violations?: string[] };
  context?: Record<string, unknown>;
  unreadable?: boolean;
  /** A command waits for this before the server answers. */
  gate?: Promise<void>;
  /** The sheet cannot be read again once a command was sent. */
  unreadableAfterCommand?: boolean;
  /** Changes the server's sheets before the first read. */
  seed?: (store: Sheet[]) => void;
  /** The address of the page; the sheet reads `?id=` / `?new=` from it and writes the open sheet back. */
  url?: string;
  /** A context value another Micro-UI publishes while the first lists are being read. */
  publishDuringFirstLists?: [name: string, value: unknown];
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** A shell over a small server: the sheets of the mock-up's lists, and commands that change them as the DataCapabilities do. */
function fakeShell(options: Options = {}) {
  const store = structuredClone([...characters, ...places]);
  options.seed?.(store);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables: Record<string, unknown> | undefined }> = [];
  const log: string[] = [];
  const contextSets: Array<[string, unknown]> = [];
  const apply =
    options.apply ??
    ((command: Sent, sheets: Sheet[]): { status: string; violations?: string[] } => {
      const element = sheets.find((s) => s.id === command.target.id);
      switch (command.dataCapability.split('@')[0]) {
        case 'campaign.saveEntity': {
          const given = command.payload.entity ?? {};
          if (element) Object.assign(element, given);
          else sheets.push({ ...given, id: String(given.id), kind: String(given.kind) });
          return { status: 'applied' };
        }
        case 'ai.proposeCharacter':
          if (element) Object.assign(element, { prompt: command.payload.prompt, background: 'Chassé de sa tribu.', behavior: 'Parle vite.', pvMax: 21, stats: { ...element.stats, DEX: 15 } });
          return { status: 'applied' };
        case 'dndbeyond.resyncCharacter':
          if (element) Object.assign(element, { pvMax: 52, stats: { ...element.stats, SAG: 16 }, aliases: ['ne doit pas bouger'] });
          return { status: 'applied' };
        default:
          return { status: 'applied' };
      }
    });
  const context = new Map<string, unknown>(Object.entries(options.context ?? { campaign: 'cendres-valombre' }));
  const listeners = new Set<(name: string) => void>();
  const shell: Shell = {
    async capability(id, _sha256, variables) {
      reads.push({ id, variables });
      log.push(`read ${id}${variables && 'id' in variables ? ` ${String(variables.id)}` : ''}`);
      if (options.unreadable || (options.unreadableAfterCommand && sent.length > 0)) throw new Error('down');
      if (id === 'campaign.getEntity') return { asOf: 1, data: answer(store.find((s) => s.id === variables?.id)) };
      if (id === 'campaign.listPlayers') {
        if (options.publishDuringFirstLists && reads.filter((r) => r.id === id).length === 1) shell.context?.set(...options.publishDuringFirstLists);
        return { asOf: 1, data: players };
      }
      if (id === 'media.listMedia') return { asOf: 1, data: media };
      throw new Error(`unknown ${id}`);
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      log.push(`command ${command.dataCapability}`);
      await options.gate;
      const outcome = apply(command, store);
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: 'applied', violations: [] } };
    },
    context: {
      get: (name) => context.get(name),
      set: (name, value) => {
        contextSets.push([name, value]);
        context.set(name, value);
        for (const listener of [...listeners]) listener(name);
      },
      on: (listener) => {
        listeners.add(listener);
        return () => void listeners.delete(listener);
      },
    },
  };
  return { shell, sent, reads, log, store, contextSets };
}

/** Lets every pending promise of the sheet finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function sheet(entity = 'morlox', options?: Options, dictation?: { transcript(): string }) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="EntityEditor"></div></body></html>', { url: options?.url });
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const events: Array<{ type: string; detail: Record<string, unknown> }> = [];
  for (const type of ['entity-editor:opened', 'entity-editor:updated', 'entity-editor:history']) host.addEventListener(type, (e) => events.push({ type, detail: (e as CustomEvent<Record<string, unknown>>).detail }));
  const mounted: Mounted = mount(host, { entity }, fake.shell, timing, dictation);
  await settle();
  const $ = <T extends HTMLElement = HTMLElement>(selector: string): T => {
    const found = document.querySelector<T>(selector);
    assert.ok(found, `${selector} is on the page`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const type = async (selector: string, value: string): Promise<void> => {
    const field = $<HTMLInputElement | HTMLTextAreaElement>(selector);
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    await settle();
  };
  const choose = async (selector: string, value: string): Promise<void> => {
    const field = $<HTMLInputElement | HTMLSelectElement>(selector);
    field.value = value;
    field.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
    await settle();
  };
  const barShown = (): boolean => !$('#entity-unsaved').hidden;
  const value = (selector: string): string => $<HTMLInputElement | HTMLTextAreaElement>(selector).value;
  return { dom, document, host, mounted, events, $, click, type, choose, barShown, value, ...fake };
}

test('the sheet of an NPC: identity, the stats with their derived modifier, the statblock, the token', async () => {
  const s = await sheet('morlox');
  assert.equal(s.$('#entity-title').textContent, 'Morlox');
  assert.equal(s.value('#entity-name'), 'Morlox');
  assert.match(s.value('#entity-description'), /./);
  // dex-modifier-derived: the DEX label shows the modifier of the score, and it is not a field.
  const dex = Number(s.value('#entity-stat-DEX'));
  assert.match(s.$('#entity-stat-DEX').closest('label')?.firstChild?.textContent ?? '', new RegExp(`^DEX ${dex >= 10 ? '\\+' : '−'}${Math.abs(Math.floor((dex - 10) / 2))}$`));
  assert.equal(s.document.querySelector('[id*="modifier"], [name*="modifier"]'), null);
  assert.ok(s.$('#entity-statblock .kit-statblock'));
  assert.equal(s.$('#entity-ddb').hidden, true);
  assert.deepEqual(
    s.reads.map((r) => [r.id, r.variables]),
    [
      ['campaign.getEntity', { id: 'morlox' }],
      ['campaign.listPlayers', { campaign: 'cendres-valombre' }],
      ['media.listMedia', { campaign: 'cendres-valombre' }],
    ],
  );
  assert.deepEqual(s.events, [{ type: 'entity-editor:opened', detail: { id: 'morlox', name: 'Morlox', kind: 'pnj' } }]);
  assert.deepEqual(s.contextSets, [['entity', 'morlox']]);
});

test('default-token: with no portrait and no token image, a disc of the colour with the initial', async () => {
  const bandit = await sheet('bandit');
  const token = bandit.$('#entity-token-preview .kit-token__disc');
  assert.equal(token.getAttribute('style'), '--kit-token-bg: var(--color-danger)');
  assert.equal(token.querySelector('.kit-token__initials')?.textContent, 'BA');
  assert.equal(token.querySelector('.kit-token__portrait'), null);
  // A sheet with a portrait shows the portrait's disc instead.
  const elowen = await sheet('elowen');
  assert.ok(elowen.$('#entity-token-preview .kit-token__portrait'));
  assert.equal(elowen.document.querySelector('#entity-token-preview .kit-token__initials'), null);
});

test('its parts name what they show and what they trigger', async () => {
  const s = await sheet('elowen');
  assert.equal(s.$('#entity-editor').dataset.reads, 'campaign.getEntity');
  assert.equal(s.$('#entity-player-field').dataset.reads, 'campaign.listPlayers');
  assert.equal(s.$('#entity-media').dataset.reads, 'media.listMedia');
  const writers = [...s.document.querySelectorAll<HTMLElement>('[data-writes]')].map((w) => [w.id, w.dataset.writes]);
  assert.deepEqual(writers, [
    ['entity-unsaved-save', 'campaign.saveEntity'],
    ['entity-propose', 'ai.proposeCharacter'],
    ['entity-ddb-import', 'dndbeyond.importCharacter'],
    ['entity-ddb-resync', 'dndbeyond.resyncCharacter'],
    ['entity-save', 'campaign.saveEntity'],
  ]);
});

test('explicit-save-only: typing writes nothing; the save button writes the sheet through campaign.saveEntity', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-description', 'Un autre marchand.');
  await s.type('#entity-aliases', 'le gobelin, Mor');
  assert.equal(s.sent.length, 0);
  await s.click('#entity-save');
  assert.equal(s.sent.length, 1);
  const [command] = s.sent;
  assert.equal(command?.dataCapability, 'campaign.saveEntity@1');
  assert.deepEqual(command?.target, { aggregate: 'Entity', id: 'morlox' });
  assert.equal(command?.payload.entity?.description, 'Un autre marchand.');
  assert.deepEqual(command?.payload.entity?.aliases, ['le gobelin', 'Mor']);
  assert.ok(!('campaign' in command.payload));
  assert.ok(!('modifiers' in (command?.payload.entity ?? {})));
  assert.match(command?.idempotencyKey ?? '', /^EntityEditor:campaign\.saveEntity:morlox:/);
  assert.deepEqual(command?.caller.roles, ['role:approver']);
  assert.match(s.$('#entity-status').textContent ?? '', /Fiche enregistrée/);
  // Saved: the sheet is not modified any more, so the rail opens another at once.
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), false);
  assert.equal(s.$('#entity-title').textContent, 'Gaspard Grosmollet');
});

test('a sheet that is not modified opens the element the rail asks for at once, with no bar', async () => {
  const s = await sheet('morlox');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), false);
  assert.equal(s.value('#entity-name'), 'Gaspard Grosmollet');
  assert.equal(s.sent.length, 0);
  assert.deepEqual(s.contextSets.at(-1), ['entity', 'gaspard']);
  assert.deepEqual(s.events.at(-1), { type: 'entity-editor:opened', detail: { id: 'gaspard', name: 'Gaspard Grosmollet', kind: 'pnj' } });
});

test('asking for the element already on show changes nothing', async () => {
  const s = await sheet('morlox');
  const reads = s.reads.length;
  s.mounted.update({ entity: 'morlox' });
  await settle();
  assert.equal(s.reads.length, reads);
  assert.equal(s.barShown(), false);
});

test('a modified sheet asks first: « Rester » keeps the sheet and the change', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-description', 'Modifiée, pas enregistrée.');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), true);
  assert.match(s.$('#entity-unsaved-text').textContent ?? '', /Morlox.*modifications non enregistrées/);
  assert.deepEqual([...s.document.querySelectorAll('#entity-unsaved button')].map((b) => b.textContent), ['Enregistrer et continuer', 'Abandonner', 'Rester']);
  assert.equal(s.$('#entity-unsaved-save').dataset.writes, 'campaign.saveEntity');
  // Nothing is read or written for the element asked for.
  assert.ok(!s.log.includes('read campaign.getEntity gaspard'));
  await s.click('#entity-unsaved-stay');
  assert.equal(s.barShown(), false);
  assert.equal(s.value('#entity-description'), 'Modifiée, pas enregistrée.');
  assert.equal(s.$('#entity-title').textContent, 'Morlox');
  assert.equal(s.sent.length, 0);
  // The rail is told the sheet that stayed is the one on show.
  assert.deepEqual(s.contextSets.at(-1), ['entity', 'morlox']);
  // Still modified: asking again asks again.
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), true);
});

test('« Abandonner » opens the element asked for and loses the change', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-description', 'Perdue.');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  await s.click('#entity-unsaved-discard');
  assert.equal(s.barShown(), false);
  assert.match(s.value('#entity-name'), /^Gaspard/);
  assert.equal(s.sent.length, 0);
  // Coming back to the first sheet, the change is not there.
  s.mounted.update({ entity: 'morlox' });
  await settle();
  assert.notEqual(s.value('#entity-description'), 'Perdue.');
  assert.equal(s.barShown(), false);
});

test('« Enregistrer et continuer » saves through campaign.saveEntity, then opens the element asked for', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-description', 'Enregistrée avant de partir.');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  await s.click('#entity-unsaved-save');
  const saved = s.log.indexOf('command campaign.saveEntity@1');
  const opened = s.log.indexOf('read campaign.getEntity gaspard');
  assert.ok(saved >= 0 && opened > saved, `the save comes before the read of the next sheet: ${s.log.join(' | ')}`);
  assert.equal(s.sent[0]?.payload.entity?.description, 'Enregistrée avant de partir.');
  assert.equal(s.barShown(), false);
  assert.match(s.value('#entity-name'), /^Gaspard/);
  assert.equal(s.store.find((e) => e.id === 'morlox')?.description, 'Enregistrée avant de partir.');
});

test('a save that is refused keeps the sheet and the choice: nothing opens', async () => {
  const s = await sheet('morlox', { apply: () => ({ status: 'rejected', violations: ['alias-unique-in-campaign'] }) });
  await s.type('#entity-aliases', 'Gaspard');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  await s.click('#entity-unsaved-save');
  assert.equal(s.barShown(), true);
  assert.equal(s.$('#entity-title').textContent, 'Morlox');
  assert.match(s.$('#entity-status').textContent ?? '', /refusé : un nom ou un alias/);
  assert.ok(!s.log.includes('read campaign.getEntity gaspard'));
  await s.click('#entity-unsaved-stay');
  assert.equal(s.value('#entity-aliases'), 'Gaspard');
});

test('a name is required, and a sheet that cannot be read says so', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-name', '   ');
  await s.click('#entity-save');
  assert.equal(s.sent.length, 0);
  assert.match(s.$('#entity-status').textContent ?? '', /Donnez un nom/);
  const down = await sheet('morlox', { unreadable: true });
  assert.match(down.$('#entity-status').textContent ?? '', /Impossible de lire la fiche/);
  const unknown = await sheet('nobody');
  assert.match(unknown.$('#entity-status').textContent ?? '', /introuvable/);
});

test('the AI proposal: ai.proposeCharacter is triggered with the prompt, the sheet shows it, « Mis à jour » leads to the history', async () => {
  const s = await sheet('morlox', undefined, { transcript: () => 'Un gobelin marchand, nerveux.' });
  // The prompt is dictated: a click starts, a click pauses and hands the transcript over.
  await s.type('#entity-prompt-text', '');
  await s.click('#entity-prompt-mic');
  assert.equal(s.$('#entity-prompt-mic-box').dataset.state, 'recording');
  await s.click('#entity-prompt-mic');
  assert.equal(s.$('#entity-prompt-mic-box').dataset.state, 'paused');
  assert.equal(s.value('#entity-prompt-text'), 'Un gobelin marchand, nerveux.');
  await s.click('#entity-propose');
  const [command] = s.sent;
  assert.equal(command?.dataCapability, 'ai.proposeCharacter@1');
  assert.deepEqual(command?.caller.roles, ['role:approver']);
  assert.deepEqual(command?.target, { aggregate: 'Entity', id: 'morlox' });
  assert.deepEqual(command?.payload, { prompt: 'Un gobelin marchand, nerveux.' });
  assert.equal(s.value('#entity-background'), 'Chassé de sa tribu.');
  assert.equal(s.value('#entity-behavior'), 'Parle vite.');
  assert.equal(s.value('#entity-pv'), '21');
  assert.equal(s.value('#entity-stat-DEX'), '15');
  assert.match(s.$('#entity-stat-DEX').closest('label')?.firstChild?.textContent ?? '', /^DEX \+2$/);
  assert.match(s.$('#entity-statblock').textContent ?? '', /21/);
  const notice = s.$('#entity-notice');
  assert.equal(notice.hidden, false);
  assert.match(notice.textContent ?? '', /Mis à jour/);
  assert.match(notice.textContent ?? '', /Background/);
  const updated = s.events.find((e) => e.type === 'entity-editor:updated');
  assert.equal(updated?.detail.origin, 'ia');
  assert.equal(updated?.detail.id, 'morlox');
  // The proposal is versioned by the server and applied at once: the sheet is not modified.
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), false);
  // « Mis à jour » leads to the diff: the host opens the history.
  const back = await sheet('morlox');
  await back.type('#entity-prompt-text', 'Un gobelin.');
  await back.click('#entity-propose');
  await back.click('#entity-notice-diff');
  assert.deepEqual(back.events.at(-1), { type: 'entity-editor:history', detail: { id: 'morlox' } });
});

test('a proposal without a prompt sends nothing; a sheet not saved has no one to ask for', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-prompt-text', '   ');
  await s.click('#entity-propose');
  assert.equal(s.sent.length, 0);
  const fresh = await sheet('new:pnj');
  assert.equal(fresh.$<HTMLButtonElement>('#entity-propose').disabled, true);
  assert.match(fresh.$('#entity-prompt').textContent ?? '', /Enregistrez la fiche/);
});

test('the AI keeps what the DM typed elsewhere: the proposal is applied, the unsaved note stays unsaved', async () => {
  const s = await sheet('morlox');
  await s.type('#entity-notes', 'Note tapée avant la proposition.');
  await s.type('#entity-prompt-text', 'Un gobelin.');
  await s.click('#entity-propose');
  assert.equal(s.value('#entity-notes'), 'Note tapée avant la proposition.');
  assert.equal(s.value('#entity-background'), 'Chassé de sa tribu.');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), true, 'the note was never saved');
});

test('a proposal the server refuses says why and changes nothing', async () => {
  const s = await sheet('morlox', { apply: () => ({ status: 'rejected', violations: ['proposal-fills-sheet-fields'] }) });
  const background = s.value('#entity-background');
  await s.type('#entity-prompt-text', 'Un gobelin.');
  await s.click('#entity-propose');
  assert.match(s.$('#entity-status').textContent ?? '', /La proposition refusé : la proposition ne remplit pas/);
  assert.equal(s.$('#entity-notice').hidden, true);
  assert.equal(s.value('#entity-background'), background);
});

test('the D&D Beyond resync of a PC: dndbeyond.resyncCharacter is triggered; aliases, notes and token are unchanged', async () => {
  const s = await sheet('elowen');
  const aliases = s.value('#entity-aliases');
  const notes = s.value('#entity-notes');
  const zoom = s.value('#entity-crop-zoom');
  assert.equal(s.$<HTMLButtonElement>('#entity-ddb-resync').disabled, false);
  assert.equal(s.$<HTMLInputElement>('#entity-ddb-url').value, 'https://www.dndbeyond.com/characters/112847390');
  await s.click('#entity-ddb-resync');
  const [command] = s.sent;
  assert.equal(command?.dataCapability, 'dndbeyond.resyncCharacter@1');
  assert.deepEqual(command?.target, { aggregate: 'Entity', id: 'elowen' });
  assert.deepEqual(command?.payload, {});
  assert.equal(s.value('#entity-pv'), '52');
  assert.equal(s.value('#entity-stat-SAG'), '16');
  // The server's resync scope is the stats, the hit points and the spells: the sheet keeps the rest as the DM has it.
  assert.equal(s.value('#entity-notes'), notes);
  assert.equal(s.value('#entity-crop-zoom'), zoom);
  assert.equal(s.value('#entity-aliases'), 'ne doit pas bouger', 'what the server holds is what the sheet shows');
  assert.ok(aliases.length > 0);
  assert.match(s.$('#entity-ddb-diff .kit-diffview__title').textContent ?? '', /Resynchronisation/);
  assert.match(s.$('#entity-ddb-kept').textContent ?? '', /Notes : /);
  assert.match(s.$('#entity-notice').textContent ?? '', /Mis à jour/);
  assert.equal(s.events.find((e) => e.type === 'entity-editor:updated')?.detail.origin, 'dndbeyond');
});

test('a resync that touches only its scope leaves the aliases, notes and token shown unchanged', async () => {
  const s = await sheet('elowen', {
    apply: (command, store) => {
      const element = store.find((e) => e.id === command.target.id);
      if (element) Object.assign(element, { pvMax: 52, stats: { ...element.stats, SAG: 16 } });
      return { status: 'applied' };
    },
  });
  const before = ['#entity-aliases', '#entity-notes', '#entity-crop-zoom', '#entity-crop-x', '#entity-crop-y'].map((id) => s.value(id));
  const token = s.$('#entity-token-preview').innerHTML;
  await s.click('#entity-ddb-resync');
  assert.deepEqual(['#entity-aliases', '#entity-notes', '#entity-crop-zoom', '#entity-crop-x', '#entity-crop-y'].map((id) => s.value(id)), before);
  assert.equal(s.$('#entity-token-preview').innerHTML, token);
  assert.deepEqual(s.document.querySelectorAll('#entity-ddb-diff .kit-diffview__row').length, 2);
});

test('a PC with no D&D Beyond link cannot resync; « Importer » triggers dndbeyond.importCharacter with the URL', async () => {
  const unlinked = (store: Sheet[]): void => {
    const brannoc = store.find((e) => e.id === 'brannoc');
    if (brannoc) brannoc.ddb = null;
  };
  const s = await sheet('brannoc', { seed: unlinked });
  assert.equal(s.$<HTMLButtonElement>('#entity-ddb-resync').disabled, true);
  assert.match(s.$('#entity-ddb-status').textContent ?? '', /Jamais importé/);
  await s.type('#entity-ddb-url', 'https://www.dndbeyond.com/characters/777');
  await s.click('#entity-ddb-import');
  const [command] = s.sent;
  assert.equal(command?.dataCapability, 'dndbeyond.importCharacter@1');
  assert.equal(command?.payload.url, 'https://www.dndbeyond.com/characters/777');
  assert.equal(command?.payload.campaign, 'cendres-valombre');
  // An empty URL sends nothing.
  const empty = await sheet('brannoc', { seed: unlinked });
  await empty.click('#entity-ddb-import');
  assert.equal(empty.sent.length, 0);
});

test('an NPC has no D&D Beyond panel; a place has no sheet, no prompt and no token', async () => {
  const gaspard = await sheet('gaspard');
  assert.equal(gaspard.$('#entity-ddb').hidden, true);
  const place = await sheet('taverne-sanglier');
  assert.equal(place.$('#entity-prompt').hidden, true);
  assert.equal(place.$('#entity-sheet').hidden, true);
  assert.equal(place.document.querySelector('#entity-token'), null);
  assert.match(place.$('#entity-illustrations').textContent ?? '', /illustration/);
  assert.equal(place.$('#entity-illustrations').dataset.reads, 'media.listMedia');
  await place.type('#entity-description', 'Une taverne.');
  await place.click('#entity-save');
  assert.deepEqual(Object.keys(place.sent[0]?.payload.entity ?? {}).sort(), ['aliases', 'description', 'id', 'kind', 'name', 'notes']);
});

test('a sheet to create: saved with the campaign and an id of its own, then it is that element', async () => {
  const s = await sheet('new:pnj');
  assert.match(s.$('.mf-entity__head').textContent ?? '', /Nouvelle fiche/);
  assert.deepEqual(s.events.at(-1), { type: 'entity-editor:opened', detail: { id: null, name: '', kind: 'pnj' } });
  await s.click('#entity-save');
  assert.equal(s.sent.length, 0, 'a sheet with no name is not saved');
  await s.type('#entity-name', 'Gobelin des ponts');
  await s.click('#entity-save');
  const [command] = s.sent;
  assert.equal(command?.payload.campaign, 'cendres-valombre');
  const id = String(command?.payload.entity?.id);
  assert.match(id, /^[0-9a-f-]{36}$/);
  assert.deepEqual(command?.target, { aggregate: 'Entity', id });
  assert.equal(command?.payload.entity?.kind, 'pnj');
  assert.equal(command?.payload.entity?.player, null);
  // It is that element now: the AI can be asked for it, and the rail follows.
  assert.equal(s.$<HTMLButtonElement>('#entity-propose').disabled, false);
  assert.deepEqual(s.events.at(-1), { type: 'entity-editor:opened', detail: { id, name: 'Gobelin des ponts', kind: 'pnj' } });
  assert.deepEqual(s.contextSets.at(-1), ['entity', id]);
});

test('a sheet to create without a campaign is not saved', async () => {
  const s = await sheet('new:lieu', { context: {} });
  await s.type('#entity-name', 'Un lieu');
  await s.click('#entity-save');
  assert.equal(s.sent.length, 0);
  assert.match(s.$('#entity-status').textContent ?? '', /Aucune campagne/);
});

test('changing a character from NPC to PC shows the player; a PC chosen by the DM is saved with its player', async () => {
  const s = await sheet('morlox');
  assert.equal(s.$('#entity-player-field').hidden, true);
  await s.choose('input[name="entity-kind"][value="pj"]', 'pj');
  assert.equal(s.$('#entity-player-field').hidden, false);
  assert.equal(s.$('#entity-ddb').hidden, false);
  await s.choose('#entity-player', 'jou-marc');
  assert.match(s.$('.mf-entity__head').textContent ?? '', /Joué par Marc/);
  await s.click('#entity-save');
  assert.equal(s.sent[0]?.payload.entity?.kind, 'pj');
  assert.equal(s.sent[0]?.payload.entity?.player, 'jou-marc');
});

test('the sheet is edited in place: features added and removed, the statblock follows the scores', async () => {
  const s = await sheet('morlox');
  const count = s.document.querySelectorAll('#entity-features li').length;
  await s.type('#entity-feature-name', 'Boule de feu');
  await s.type('#entity-feature-text', '8d6');
  await s.click('#entity-feature-submit');
  assert.equal(s.document.querySelectorAll('#entity-features li').length, count + 1);
  assert.equal(s.value('#entity-feature-name'), '');
  assert.match(s.$('#entity-statblock').textContent ?? '', /Boule de feu/);
  await s.click('#entity-features li:last-child [data-remove-feature]');
  assert.equal(s.document.querySelectorAll('#entity-features li').length, count);
  await s.type('#entity-stat-FOR', '18');
  assert.match(s.$('#entity-stat-FOR').closest('label')?.firstChild?.textContent ?? '', /^FOR \+4$/);
  assert.match(s.$('.kit-statblock__ability').textContent ?? '', /18\+4/);
  // Back to the stored scores: the sheet is not modified.
  await s.type('#entity-stat-FOR', String(characters.find((c) => c.id === 'morlox')?.stats?.FOR));
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), false);
});

test('a portrait and a token image are chosen in the library, and only a choice is sent', async () => {
  const s = await sheet('morlox');
  await s.click('#entity-save');
  assert.ok(!('portrait' in (s.sent[0]?.payload.entity ?? {})) && !('token' in (s.sent[0]?.payload.entity ?? {})));
  await s.choose('#entity-portrait', 'ill-cover');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), true);
  await s.click('#entity-unsaved-save');
  assert.equal(s.sent[1]?.payload.entity?.portrait, 'ill-cover');
});

test('the sheet follows its props: a prop with no element asks for nothing', async () => {
  const s = await sheet('morlox');
  s.mounted.update({ entity: '' });
  s.mounted.update({});
  await settle();
  assert.equal(s.$('#entity-title').textContent, 'Morlox');
  const none = await sheet('');
  assert.match(none.$('#entity-status').textContent ?? '', /Aucune fiche/);
});

test('after a refused save the bar and the sheet answer again', async () => {
  const s = await sheet('morlox', { apply: () => ({ status: 'rejected', violations: ['alias-unique-in-campaign'] }) });
  await s.type('#entity-aliases', 'Gaspard');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  await s.click('#entity-unsaved-save');
  assert.equal(s.$<HTMLButtonElement>('#entity-unsaved-stay').disabled, false);
  assert.equal(s.$<HTMLInputElement>('#entity-name').disabled, false);
  assert.equal(s.$<HTMLButtonElement>('#entity-save').disabled, false);
  await s.click('#entity-unsaved-discard');
  assert.equal(s.$('#entity-title').textContent, 'Gaspard Grosmollet');
});

test('a request of the rail while a command runs waits for it: a sheet the command leaves clean opens with no bar', async () => {
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (release = resolve));
  const s = await sheet('morlox', { gate });
  await s.type('#entity-prompt-text', 'Un gobelin.');
  await s.click('#entity-propose');
  s.mounted.update({ entity: 'gaspard' });
  await settle();
  assert.equal(s.barShown(), false);
  assert.ok(!s.log.includes('read campaign.getEntity gaspard'));
  release();
  await settle();
  assert.equal(s.barShown(), false);
  assert.equal(s.$('#entity-title').textContent, 'Gaspard Grosmollet');
});

test('a proposal applied whose sheet cannot be read again says so, and does not say it failed', async () => {
  const s = await sheet('morlox', { unreadableAfterCommand: true });
  await s.type('#entity-prompt-text', 'Un gobelin.');
  await s.click('#entity-propose');
  assert.match(s.$('#entity-status').textContent ?? '', /a abouti, mais la fiche n’a pas pu être relue/);
  assert.equal(s.$<HTMLButtonElement>('#entity-save').disabled, false);
});

/** What the sheet answers on the page's bus, and the address it writes. */
const busOf = (s: Awaited<ReturnType<typeof sheet>>) => {
  const opened: Array<{ id: string | null; name: string; kind: string }> = [];
  s.document.addEventListener('fdm:entity-opened', (e) => opened.push((e as CustomEvent<(typeof opened)[number]>).detail));
  const ask = async (name: string, detail?: unknown): Promise<void> => {
    s.document.dispatchEvent(new s.dom.window.CustomEvent(name, { detail }));
    await settle();
  };
  return { opened, ask };
};

test('the page\'s bus: the rail asks for a sheet by its id or for a new one, and the sheet opens in place', async () => {
  const s = await sheet('morlox');
  const bus = busOf(s);
  await bus.ask('fdm:open-entity', { id: 'gaspard' });
  assert.equal(s.$('#entity-title').textContent, 'Gaspard Grosmollet');
  await bus.ask('fdm:open-entity', { new: 'lieu' });
  assert.equal(s.value('#entity-name'), '');
  assert.equal(s.$('#entity-save').textContent, 'Enregistrer la fiche');
  // A request naming neither asks for nothing.
  await bus.ask('fdm:open-entity', {});
  await bus.ask('fdm:open-entity');
  assert.equal(s.value('#entity-name'), '');
});

test('the page\'s bus: a modified sheet answers the rail with the bar, then opens what the DM chose', async () => {
  const s = await sheet('morlox');
  const bus = busOf(s);
  await s.type('#entity-description', 'Changée.');
  await bus.ask('fdm:open-entity', { id: 'gaspard' });
  assert.equal(s.barShown(), true);
  assert.equal(s.value('#entity-name'), 'Morlox');
  await s.click('#entity-unsaved-discard');
  assert.equal(s.$('#entity-title').textContent, 'Gaspard Grosmollet');
});

test('the page\'s bus: the sheet on show is announced when it opens and when the rail asks (entity-query)', async () => {
  const s = await sheet('morlox');
  const bus = busOf(s);
  await bus.ask('fdm:open-entity', { id: 'gaspard' });
  assert.deepEqual(bus.opened.map((o) => o.id), ['gaspard']);
  assert.equal(bus.opened[0]?.name, 'Gaspard Grosmollet');
  await bus.ask('fdm:entity-query');
  assert.deepEqual(bus.opened.map((o) => o.id), ['gaspard', 'gaspard']);
});

test('the address names the open sheet, in the same document: ?id= for a saved one, ?new= for one to create, the rest of the query stays', async () => {
  const s = await sheet('morlox', { url: 'http://localhost/screens/fiche-entite?id=morlox&campaign=cendres-valombre#haut' });
  const view = s.dom.window;
  const bus = busOf(s);
  await bus.ask('fdm:open-entity', { id: 'gaspard' });
  assert.equal(view.location.search, '?campaign=cendres-valombre&id=gaspard');
  assert.equal(view.location.pathname, '/screens/fiche-entite');
  assert.equal(view.location.hash, '#haut');
  await bus.ask('fdm:open-entity', { new: 'pnj' });
  assert.equal(view.location.search, '?campaign=cendres-valombre&new=pnj');
});

test('a sheet created takes its id into the address once saved', async () => {
  const s = await sheet('new:pnj', { url: 'http://localhost/screens/fiche-entite?new=pnj', context: { campaign: 'cendres-valombre' } });
  const bus = busOf(s);
  await s.type('#entity-name', 'Zorbak');
  await s.click('#entity-save');
  assert.equal(bus.opened.length, 1);
  assert.match(s.dom.window.location.search, /^\?id=[0-9a-f-]{36}$/);
  assert.equal(bus.opened[0]?.id, new URLSearchParams(s.dom.window.location.search).get('id'));
});

test('with no prop, the first sheet is the address\'s: ?id= for an element, ?new= for one to create; the prop wins over the address', async () => {
  const byId = await sheet('', { url: 'http://localhost/screens/fiche-entite?id=gaspard' });
  assert.equal(byId.$('#entity-title').textContent, 'Gaspard Grosmollet');
  const fresh = await sheet('', { url: 'http://localhost/screens/fiche-entite?new=lieu' });
  assert.equal(fresh.value('#entity-name'), '');
  const both = await sheet('morlox', { url: 'http://localhost/screens/fiche-entite?id=gaspard' });
  assert.equal(both.$('#entity-title').textContent, 'Morlox');
  const none = await sheet('', { url: 'http://localhost/screens/fiche-entite' });
  assert.match(none.$('#entity-status').textContent ?? '', /Aucune fiche/);
  // An empty ?id= names nothing: the ?new= beside it does.
  const empty = await sheet('', { url: 'http://localhost/screens/fiche-entite?id=&new=lieu' });
  assert.equal(empty.value('#entity-name'), '');
  assert.doesNotMatch(empty.$('#entity-status').textContent ?? '', /Aucune fiche/);
});

test('a sheet that left the page stops answering the bus', async () => {
  const s = await sheet('morlox');
  const bus = busOf(s);
  s.host.remove();
  await bus.ask('fdm:open-entity', { id: 'gaspard' });
  await bus.ask('fdm:entity-query');
  assert.deepEqual(bus.opened, []);
  assert.equal(s.host.querySelector<HTMLInputElement>('#entity-name')?.value, 'Morlox');
});

test('a campaign published after the sheet opened reaches its lists: the players and the media are read again for it', async () => {
  const s = await sheet('new:pnj', { context: {} });
  const listed = () => s.reads.filter((r) => r.id === 'campaign.listPlayers' || r.id === 'media.listMedia');
  assert.deepEqual(listed().map((r) => r.variables), [{}, {}]);
  // Typed, the change not yet told to the sheet: the lists' repaint keeps it all the same.
  s.$<HTMLInputElement>('#entity-name').value = 'Zorbak';
  s.shell.context?.set('campaign', 'cendres-valombre');
  await settle();
  assert.deepEqual(listed().map((r) => r.variables), [{}, {}, { campaign: 'cendres-valombre' }, { campaign: 'cendres-valombre' }]);
  // What the DM typed meanwhile stays.
  assert.equal(s.value('#entity-name'), 'Zorbak');
  // Another name of the context, or the same campaign again, reads nothing.
  s.shell.context?.set('entity', 'x');
  s.shell.context?.set('campaign', 'cendres-valombre');
  await settle();
  assert.equal(listed().length, 4);
});

test('a campaign published while the first lists are read is read for as well: the rail and the sheet mount in no set order', async () => {
  const s = await sheet('new:pnj', { context: {}, publishDuringFirstLists: ['campaign', 'cendres-valombre'] });
  const listed = s.reads.filter((r) => r.id === 'campaign.listPlayers' || r.id === 'media.listMedia');
  assert.deepEqual(listed.map((r) => r.variables), [{}, {}, { campaign: 'cendres-valombre' }, { campaign: 'cendres-valombre' }]);
});

test('a sheet that left the page stops following the context', async () => {
  const s = await sheet('new:pnj', { context: {} });
  s.host.remove();
  s.shell.context?.set('campaign', 'cendres-valombre');
  await settle();
  assert.equal(s.reads.filter((r) => r.id === 'campaign.listPlayers').length, 1);
});

test('a campaign published while a command runs is read all the same; the sheet stays locked until the command ends', async () => {
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (release = resolve));
  const s = await sheet('new:pnj', { context: {}, gate });
  await s.type('#entity-name', 'Zorbak');
  s.shell.context?.set('campaign', 'cendres-valombre');
  await s.click('#entity-save');
  s.shell.context?.set('campaign', 'autre');
  await settle();
  assert.equal(s.$<HTMLInputElement>('#entity-name').disabled, true);
  assert.deepEqual(s.reads.filter((r) => r.id === 'campaign.listPlayers').map((r) => r.variables), [{}, { campaign: 'cendres-valombre' }, { campaign: 'autre' }]);
  release();
  await settle();
  assert.equal(s.$<HTMLInputElement>('#entity-name').disabled, false);
});

test('a sheet renamed and saved is announced under its new name when the rail asks', async () => {
  const s = await sheet('morlox');
  const bus = busOf(s);
  await s.type('#entity-name', 'Morlox l’Ancien');
  await s.click('#entity-save');
  await bus.ask('fdm:entity-query');
  assert.deepEqual(bus.opened.map((o) => o.name), ['Morlox l’Ancien']);
});
