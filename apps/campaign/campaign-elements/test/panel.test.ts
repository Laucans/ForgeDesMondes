import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { domBus, mount } from '../src/index.ts';
import { FOLD_KEY } from '../src/model.ts';
import type { Bus, OpenRequest, Opened, Props, Shell } from '../src/types.ts';

const IDS = ['campaign.listPlayers', 'campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents', 'media.listMedia', 'soundbox.listSounds'];

/** The mock-up's fixtures of the six Capabilities: what the panel is rendered on. */
const fixture = (id: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${id}.json`, import.meta.url), 'utf8'));
const DATA = Object.fromEntries(IDS.map((id) => [id, fixture(id)]));

interface Options {
  fail?: string[];
  data?: Record<string, unknown>;
  narrow?: boolean;
  url?: string;
  storage?: Record<string, string>;
}

function fakeShell(options: Options = {}) {
  const reads: Array<{ id: string; sha256: string | undefined; variables: Record<string, unknown> | undefined }> = [];
  let listener: (() => void) | undefined;
  const data = structuredClone({ ...DATA, ...options.data });
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      if (options.fail?.includes(id)) throw new Error('down');
      return { asOf: 1, data: structuredClone(data[id]) };
    },
    onDataVersion(next) {
      listener = () => next(1);
      return () => undefined;
    },
  };
  return { shell, reads, data, moved: () => listener?.() };
}

function fakeBus() {
  const opened: OpenRequest[] = [];
  const left: string[] = [];
  let asked = 0;
  let tell: ((o: Opened) => void) | undefined;
  const bus: Bus = {
    open: (request) => void opened.push(request),
    navigate: (href) => void left.push(href),
    query: () => void (asked += 1),
    onOpened(listener) {
      tell = listener;
      return () => undefined;
    },
  };
  return { bus, opened, left, asked: () => asked, tell: (o: Opened) => tell?.(o) };
}

const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

/** A page with a host; `narrow` makes the viewport one the drawer is for. */
function page(options: Options = {}) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignElements"></div></body></html>', { url: options.url ?? 'http://localhost/fiche-entite.html' });
  const { window } = dom;
  const media = { matches: options.narrow === true, listeners: new Set<() => void>() };
  Object.defineProperty(window, 'matchMedia', {
    value: (query: string) => ({
      get matches() {
        return media.matches && query.includes('960');
      },
      addEventListener: (_: string, fn: () => void) => media.listeners.add(fn),
      removeEventListener: (_: string, fn: () => void) => media.listeners.delete(fn),
    }),
  });
  for (const [key, value] of Object.entries(options.storage ?? {})) window.localStorage.setItem(key, value);
  return { dom, window, document: window.document, host: window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement, media };
}

async function panel(props: Props, options: Options = {}) {
  const p = page(options);
  const shell = fakeShell(options);
  const bus = fakeBus();
  const mounted = mount(p.host, props, shell.shell, bus.bus);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = p.document.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the page`);
    return found;
  };
  const click = async (selector: string, init: MouseEventInit = {}): Promise<void> => {
    $(selector).dispatchEvent(new p.window.MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  const type = async (text: string): Promise<void> => {
    const input = $('#rail-search-input') as HTMLInputElement;
    input.value = text;
    input.dispatchEvent(new p.window.Event('input', { bubbles: true }));
    await settle();
  };
  const names = (): string[] => [...p.document.querySelectorAll('.mf-rail__entry b')].map((b) => b.textContent ?? '');
  const ids = (): string[] => [...p.document.querySelectorAll<HTMLElement>('.mf-rail__entry')].map((a) => a.dataset.entity ?? '');
  return { ...p, shell, bus, mounted, $, click, type, names, ids };
}

// ---- tabs

test('tabs mode (the default) reads the six Capabilities by identifier, once each, and no hash is sent', async () => {
  const t = await panel({});
  assert.deepEqual(t.shell.reads.map((r) => r.id).sort(), [...IDS].sort());
  assert.ok(t.shell.reads.every((r) => r.sha256 === undefined));
});

test('tabs mode: six tabs list players, characters, places, events, media and sounds, with their counts', async () => {
  const t = await panel({ mode: 'tabs' });
  const tabs = [...t.document.querySelectorAll('[role="tab"]')].map((tab) => tab.textContent);
  assert.deepEqual(tabs, ['Joueurs (4)', 'Personnages (9)', 'Lieux (4)', 'Événements (3)', 'Médias (7)', 'Sons (' + (DATA['soundbox.listSounds'] as { sounds: unknown[] }).sounds.length + ')']);
  assert.equal(t.document.querySelectorAll('[role="tabpanel"]').length, 6);
  assert.equal(t.$('#elements-tabs-tab-1').getAttribute('aria-selected'), 'true');
  assert.equal(t.$('#elements-tabs-panel-1').hidden, false);
  assert.equal(t.$('#elements-tabs-panel-0').hidden, true);
});

test('a tab shows its own panel only', async () => {
  const t = await panel({});
  await t.click('#elements-tabs-tab-3');
  assert.equal(t.$('#elements-tabs-tab-3').getAttribute('aria-selected'), 'true');
  assert.equal(t.$('#elements-tabs-tab-1').getAttribute('aria-selected'), 'false');
  assert.deepEqual([...t.document.querySelectorAll<HTMLElement>('[role="tabpanel"]')].map((p) => !p.hidden), [false, false, false, true, false, false].map((_, i) => i === 3));
  assert.match(t.$('#elements-tabs-panel-3').textContent ?? '', /L’incendie du moulin/);
});

test('the players tab lists each player with the characters they play, their sync and their level', async () => {
  const t = await panel({});
  const row = t.$('#player-jou-julie');
  const cells = [...row.querySelectorAll('td')].map((c) => c.textContent);
  assert.equal(cells[0], 'Julie');
  assert.equal(cells[1], 'Elowen Ventargent');
  assert.match(cells[2] ?? '', /^Synchronisé le /);
  assert.equal(cells[3], '5');
  assert.equal(row.querySelector('a')?.getAttribute('href'), 'fiche-entite.html?id=elowen');
});

test('the characters tab lists the PJ, then the PNJ under their title, each linking to its sheet', async () => {
  const t = await panel({});
  const panelEl = t.$('#elements-tabs-panel-1');
  assert.equal(panelEl.querySelectorAll('a.mf-elements__entry').length, 9);
  assert.equal(panelEl.querySelector('h3')?.textContent, 'PNJ');
  assert.equal(t.$('#elements-char-elowen').getAttribute('href'), 'fiche-entite.html?id=elowen');
  assert.match(t.$('#elements-char-elowen').textContent ?? '', /PJ · Elowen, la rôdeuse/);
  assert.match(t.$('#elements-char-morlox').textContent ?? '', /PNJ · le gobelin, le marchand, Morlo/);
});

test('places, events, media and sounds each list theirs, with the page links of the media and the sounds', async () => {
  const t = await panel({});
  assert.equal(t.document.querySelectorAll('#elements-tabs-panel-2 a.mf-elements__entry').length, 4);
  assert.equal(t.document.querySelectorAll('#elements-tabs-panel-3 a.mf-elements__entry').length, 3);
  assert.equal(t.document.querySelectorAll('#elements-tabs-panel-4 a.mf-elements__entry').length, 7);
  assert.match(t.$('#elements-media-bm-gue').textContent ?? '', /Battlemap · 2,4 Mo/);
  assert.match(t.$('#elements-media-ill-cover').textContent ?? '', /Illustration · /);
  assert.match(t.$('#elements-sound-snd-porte').textContent ?? '', /♪ Porte qui grince/);
  assert.equal(t.$('#elements-tabs-panel-4 button[data-href]').dataset.href, 'medias.html');
  assert.equal(t.$('#elements-tabs-panel-5 button[data-href]').dataset.href, 'soundbox.html');
});

test('every part that shows a read names it, and each of the six Capabilities is named', async () => {
  const t = await panel({});
  const named = new Set([...t.document.querySelectorAll('[data-reads]')].flatMap((el) => (el.getAttribute('data-reads') ?? '').split(' ')));
  assert.deepEqual([...named].sort(), [...IDS].sort());
  assert.equal(t.document.querySelectorAll('[data-writes]').length, 0);
  const own = [...t.document.querySelectorAll('[role="tabpanel"]')].map((p) => p.getAttribute('data-reads'));
  assert.deepEqual(own, ['campaign.listPlayers', 'campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents', 'media.listMedia', 'soundbox.listSounds']);
});

test('the links of the media and sounds tabs are attributes the page text cannot break', async () => {
  const t = await panel({});
  for (const button of t.document.querySelectorAll<HTMLElement>('[data-href]')) assert.doesNotMatch(button.dataset.href ?? '', /["<>]/);
});

test('« Nouveau… » in the tabs leads to the sheet of a new element; a link of the page leaves by the bus', async () => {
  const t = await panel({});
  await t.click('#elements-new-pnj');
  await t.click('#elements-new-place');
  await t.click('#elements-new-event');
  await t.click('#elements-tabs-panel-4 button[data-href]');
  assert.deepEqual(t.bus.left, ['fiche-entite.html?new=pnj', 'fiche-entite.html?new=lieu', 'fiche-entite.html?new=evenement', 'medias.html']);
  assert.deepEqual(t.bus.opened, []);
});

test('one Capability that fails leaves the other tabs on screen, and says which list is unreadable', async () => {
  const t = await panel({}, { fail: ['media.listMedia'] });
  assert.equal(t.$('#elements-tabs-tab-4').textContent, 'Médias');
  assert.match(t.$('#elements-tabs-panel-4').textContent ?? '', /Impossible de lire cette liste/);
  assert.equal(t.$('#elements-tabs-tab-2').textContent, 'Lieux (4)');
  assert.equal(t.document.querySelectorAll('#elements-tabs-panel-2 a').length, 4);
});

test('the players tab keeps its rows when the characters cannot be read, and says so', async () => {
  const t = await panel({}, { fail: ['campaign.listCharacters'] });
  assert.equal(t.$('#elements-tabs-tab-0').textContent, 'Joueurs (4)');
  assert.deepEqual([...t.$('#player-jou-julie').querySelectorAll('td')].map((c) => c.textContent), ['Julie', '', '', '']);
  assert.match(t.$('#elements-tabs-panel-0').textContent ?? '', /Impossible de lire les personnages/);
  assert.match(t.$('#elements-tabs-panel-1').textContent ?? '', /Impossible de lire/);
});

test('the campaign is asked for by its prop, else by the address', async () => {
  const byProp = await panel({ campaign: ' c-1 ' }, { url: 'http://localhost/campagne.html?campaign=from-url' });
  assert.ok(byProp.shell.reads.every((r) => r.variables?.campaign === 'c-1'));
  const byUrl = await panel({}, { url: 'http://localhost/campagne.html?campaign=from-url' });
  assert.ok(byUrl.shell.reads.every((r) => r.variables?.campaign === 'from-url'));
  const none = await panel({});
  assert.ok(none.shell.reads.every((r) => r.variables?.campaign === ''));
});

// ---- rail

test('rail mode reads the three Capabilities of its sections only, and asks the sheet which element it shows', async () => {
  const t = await panel({ mode: 'rail' });
  assert.deepEqual(t.shell.reads.map((r) => r.id).sort(), ['campaign.listCharacters', 'campaign.listEvents', 'campaign.listPlaces']);
  assert.equal(t.bus.asked(), 1);
  assert.equal(t.document.querySelector('#campaign-elements'), null);
  assert.equal(t.$('#rail-panel').getAttribute('aria-label'), 'Éléments de la campagne');
  assert.equal(t.$('#rail-panel').tagName, 'NAV');
});

test('the rail lists PJ, PNJ, places and events, each in its own section with its count', async () => {
  const t = await panel({ mode: 'rail' });
  const groups = [...t.document.querySelectorAll('.mf-rail__group')].map((g) => [g.id, g.getAttribute('aria-label'), g.querySelector('small')?.textContent]);
  assert.deepEqual(groups, [
    ['rail-group-pj', 'PJ', '4'],
    ['rail-group-pnj', 'PNJ', '5'],
    ['rail-group-place', 'Lieux', '4'],
    ['rail-group-event', 'Événements', '3'],
  ]);
  const sections = [...t.document.querySelectorAll('.mf-rail__group')].map((g) => g.getAttribute('data-reads'));
  assert.deepEqual(sections, ['campaign.listCharacters', 'campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents']);
  assert.equal(t.document.querySelector('#rail-entry-snd-porte'), null);
});

test('typing « elowen », then « rodeuse », finds Elowen both times (accent- and case-insensitive)', async () => {
  const t = await panel({ mode: 'rail' });
  await t.type('elowen');
  assert.ok(t.ids().includes('elowen'));
  assert.equal(t.$('#rail-count').textContent, '1 résultat pour « elowen »');
  await t.type('rodeuse');
  assert.ok(t.ids().includes('elowen'));
  await t.type('RÔDEUSE');
  assert.ok(t.ids().includes('elowen'));
  assert.deepEqual(t.names(), ['Elowen Ventargent']);
});

test('a search with no answer says so, and clearing it brings the whole list back', async () => {
  const t = await panel({ mode: 'rail' });
  await t.type('zzzzzz');
  assert.deepEqual(t.ids(), []);
  assert.equal(t.$('#rail-count').textContent, 'Rien pour « zzzzzz »');
  await t.type('');
  assert.equal(t.ids().length, 16);
  assert.equal(t.$('#rail-count').textContent, '');
});

test('a section that has nothing left after the search disappears; the others stay', async () => {
  const t = await panel({ mode: 'rail' });
  await t.type('tour');
  assert.equal(t.document.querySelector('#rail-group-pj'), null);
  assert.ok(t.document.querySelector('#rail-group-place'));
});

test('clicking the title « PJ » twice hides then shows the PJ, ▸ then ▾, the other sections staying open', async () => {
  const t = await panel({ mode: 'rail' });
  const caret = (g: string): string => t.$(`#rail-group-toggle-${g} .mf-rail__caret`).textContent ?? '';
  assert.equal(caret('pj'), '▾');
  await t.click('#rail-group-toggle-pj');
  assert.equal(t.$('#rail-list-pj').hidden, true);
  assert.equal(caret('pj'), '▸');
  assert.equal(t.$('#rail-group-toggle-pj').getAttribute('aria-expanded'), 'false');
  for (const other of ['pnj', 'place', 'event']) {
    assert.equal(t.$(`#rail-list-${other}`).hidden, false);
    assert.equal(caret(other), '▾');
  }
  await t.click('#rail-group-toggle-pj');
  assert.equal(t.$('#rail-list-pj').hidden, false);
  assert.equal(caret('pj'), '▾');
  assert.equal(t.$('#rail-group-toggle-pj').getAttribute('aria-expanded'), 'true');
});

test('a section folded stays folded through a search', async () => {
  const t = await panel({ mode: 'rail' });
  await t.click('#rail-group-toggle-place');
  await t.type('tour');
  assert.equal(t.$('#rail-list-place').hidden, true);
  assert.equal(t.$('#rail-group-toggle-place .mf-rail__caret').textContent, '▸');
});

test('folding the whole rail is remembered: a reload finds it folded, and unfolding is remembered too', async () => {
  const first = page();
  mount(first.host, { mode: 'rail' }, fakeShell().shell, fakeBus().bus);
  await settle();
  const rail = first.document.querySelector<HTMLElement>('#elements-rail') as HTMLElement;
  assert.equal(rail.dataset.collapsed, 'false');
  first.document.querySelector<HTMLElement>('#rail-toggle')?.click();
  assert.equal(rail.dataset.collapsed, 'true');
  assert.equal(first.window.localStorage.getItem(FOLD_KEY), '{"collapsed":true}');
  assert.equal(first.document.querySelector('#rail-toggle')?.textContent, '»');
  assert.equal(first.document.querySelector('#rail-toggle')?.getAttribute('aria-expanded'), 'false');

  // The page is read again in the same browser: a new host, the same storage.
  const reload = first.document.createElement('div');
  first.document.body.append(reload);
  mount(reload, { mode: 'rail' }, fakeShell().shell, fakeBus().bus);
  await settle();
  assert.equal(reload.querySelector<HTMLElement>('#elements-rail')?.dataset.collapsed, 'true');

  reload.querySelector<HTMLElement>('#rail-toggle')?.click();
  assert.equal(first.window.localStorage.getItem(FOLD_KEY), '{"collapsed":false}');
});

test('a rail that was left unfolded is unfolded on the next visit', async () => {
  const t = await panel({ mode: 'rail' }, { storage: { [FOLD_KEY]: '{"collapsed":false}' } });
  assert.equal(t.$('#elements-rail').dataset.collapsed, 'false');
  const folded = await panel({ mode: 'rail' }, { storage: { [FOLD_KEY]: '{"collapsed":true}' } });
  assert.equal(folded.$('#elements-rail').dataset.collapsed, 'true');
});

test('a click on an element emits the open request with its id and opens nothing itself', async () => {
  const t = await panel({ mode: 'rail' });
  const link = t.$('#rail-entry-brannoc');
  const event = new t.window.MouseEvent('click', { bubbles: true, cancelable: true });
  link.dispatchEvent(event);
  await settle();
  assert.deepEqual(t.bus.opened, [{ id: 'brannoc' }]);
  assert.equal(event.defaultPrevented, true);
  // The element is not marked current until the sheet says it is open.
  assert.equal(link.classList.contains('is-current'), false);
  assert.equal(t.bus.left.length, 0);
});

test('a click with a modifier is the browser’s own (a new tab): no open request, the click is left alone', async () => {
  const t = await panel({ mode: 'rail' });
  // What the panel left of the click when it reached the page; the page then swallows it, a test browser has nowhere to go.
  const left: boolean[] = [];
  t.document.addEventListener('click', (e) => {
    left.push(!e.defaultPrevented);
    e.preventDefault();
  });
  t.$('#rail-entry-brannoc').dispatchEvent(new t.window.MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true }));
  t.$('#rail-entry-brannoc').dispatchEvent(new t.window.MouseEvent('click', { bubbles: true, cancelable: true, button: 1 }));
  t.$('#rail-entry-brannoc').dispatchEvent(new t.window.MouseEvent('click', { bubbles: true, cancelable: true, shiftKey: true }));
  t.$('#rail-entry-brannoc').dispatchEvent(new t.window.MouseEvent('click', { bubbles: true, cancelable: true, metaKey: true }));
  await settle();
  assert.deepEqual(t.bus.opened, []);
  assert.deepEqual(left, [true, true, true, true]);
});

test('« Nouveau… » asks the sheet for a new PNJ, place or event', async () => {
  const t = await panel({ mode: 'rail' });
  await t.click('#rail-new-pnj');
  await t.click('#rail-new-place');
  await t.click('#rail-new-event');
  assert.deepEqual(t.bus.opened, [{ new: 'pnj' }, { new: 'lieu' }, { new: 'evenement' }]);
  assert.deepEqual(
    [...t.document.querySelectorAll('#rail-new button')].map((b) => b.textContent),
    ['Nouveau PNJ', 'Nouveau lieu', 'Nouvel événement'],
  );
});

test('the element the sheet shows is the current one, by the address first and then by what the sheet says', async () => {
  const t = await panel({ mode: 'rail' }, { url: 'http://localhost/fiche-entite.html?id=elowen' });
  assert.equal(t.$('#rail-entry-elowen').getAttribute('aria-current'), 'true');
  t.bus.tell({ id: 'morlox' });
  assert.equal(t.$('#rail-entry-morlox').getAttribute('aria-current'), 'true');
  assert.equal(t.$('#rail-entry-morlox').classList.contains('is-current'), true);
  assert.equal(t.$('#rail-entry-elowen').hasAttribute('aria-current'), false);
  assert.equal(t.$('#rail-entry-elowen').classList.contains('is-current'), false);
  // A new sheet, not saved yet, is no element of the list.
  t.bus.tell({ id: null });
  assert.equal(t.document.querySelectorAll('[aria-current]').length, 0);
  t.bus.tell({});
  assert.equal(t.document.querySelectorAll('.is-current').length, 0);
});

test('a search keeps the current element marked', async () => {
  const t = await panel({ mode: 'rail' });
  t.bus.tell({ id: 'elowen' });
  await t.type('elowen');
  assert.equal(t.$('#rail-entry-elowen').classList.contains('is-current'), true);
});

test('on a wide screen the drawer’s button and veil stay closed, and Escape does nothing', async () => {
  const t = await panel({ mode: 'rail' });
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.$('#rail-veil').hidden, true);
  t.$('#rail-panel').dispatchEvent(new t.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.$('#rail-toggle').textContent, '«');
});

test('under 960 px the rail is a drawer: it opens over the sheet and closes without hiding it', async () => {
  const t = await panel({ mode: 'rail' }, { narrow: true });
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.$('#rail-toggle').textContent, '✕');
  await t.click('#rail-open');
  assert.equal(t.$('#elements-rail').dataset.open, 'true');
  assert.equal(t.$('#rail-veil').hidden, false);
  assert.equal(t.$('#rail-open').getAttribute('aria-expanded'), 'true');
  assert.equal(t.$('#rail-toggle').getAttribute('aria-expanded'), 'true');
  await t.click('#rail-toggle');
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.$('#rail-veil').hidden, true);
  // Closing the drawer does not fold the rail, nor remember anything.
  assert.equal(t.$('#elements-rail').dataset.collapsed, 'false');
  assert.equal(t.window.localStorage.getItem(FOLD_KEY), null);
});

test('the drawer closes on its veil, on Escape (the focus back on its button), and after a choice', async () => {
  const t = await panel({ mode: 'rail' }, { narrow: true });
  await t.click('#rail-open');
  await t.click('#rail-veil');
  assert.equal(t.$('#elements-rail').dataset.open, 'false');

  await t.click('#rail-open');
  t.$('#rail-panel').dispatchEvent(new t.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.document.activeElement?.id, 'rail-open');

  await t.click('#rail-open');
  t.$('#rail-panel').dispatchEvent(new t.window.KeyboardEvent('keydown', { key: 'a', bubbles: true }));
  assert.equal(t.$('#elements-rail').dataset.open, 'true');
  await t.click('#rail-entry-kael');
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.deepEqual(t.bus.opened, [{ id: 'kael' }]);

  await t.click('#rail-open');
  await t.click('#rail-new-place');
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
});

test('a drawer left open is closed when the screen changes width', async () => {
  const t = await panel({ mode: 'rail' }, { narrow: true });
  await t.click('#rail-open');
  t.media.matches = false;
  for (const listener of t.media.listeners) listener();
  assert.equal(t.$('#elements-rail').dataset.open, 'false');
  assert.equal(t.$('#rail-toggle').textContent, '«');
});

test('the note of a list that could not be read stays through a search', async () => {
  const t = await panel({ mode: 'rail' }, { fail: ['campaign.listPlaces'] });
  await t.type('elowen');
  assert.match(t.$('#rail-groups').textContent ?? '', /Impossible de lire/);
  assert.deepEqual(t.names(), ['Elowen Ventargent']);
});

test('a Capability that fails leaves the other sections on screen, with a note', async () => {
  const t = await panel({ mode: 'rail' }, { fail: ['campaign.listPlaces'] });
  assert.match(t.$('#rail-groups').textContent ?? '', /Impossible de lire/);
  assert.equal(t.document.querySelector('#rail-group-place'), null);
  assert.ok(t.document.querySelector('#rail-group-pj'));
  assert.ok(t.document.querySelector('#rail-group-event'));
});

test('the lists are read again when the data moves, not on the shell’s first word', async () => {
  const t = await panel({ mode: 'rail' });
  t.shell.moved();
  await settle();
  assert.equal(t.shell.reads.length, 3);
  const characters = t.shell.data['campaign.listCharacters'] as Array<Record<string, unknown>>;
  characters.push({ id: 'nouveau', name: 'Nouvelle', aliases: [], kind: 'pnj', player: null });
  t.shell.moved();
  await settle();
  assert.equal(t.shell.reads.length, 6);
  assert.ok(t.ids().includes('nouveau'));
});

test('a panel taken off the page lets go of the data poll', async () => {
  const t = await panel({ mode: 'rail' });
  t.host.remove();
  t.shell.moved();
  await settle();
  assert.equal(t.shell.reads.length, 3);
});

test('another campaign reads the lists again; the other mode redraws the panel', async () => {
  const t = await panel({ mode: 'tabs', campaign: 'a' });
  t.mounted.update({ mode: 'tabs', campaign: 'b' });
  await settle();
  assert.equal(t.shell.reads.filter((r) => r.variables?.campaign === 'b').length, 6);
  t.mounted.update({ mode: 'rail', campaign: 'b' });
  await settle();
  assert.ok(t.document.querySelector('#elements-rail'));
  assert.equal(t.document.querySelector('#campaign-elements'), null);
  t.mounted.update({ mode: 'tabs', campaign: 'b' });
  await settle();
  assert.equal(t.document.querySelector('#elements-rail'), null);
  assert.equal(t.$('#elements-tabs-tab-1').getAttribute('aria-selected'), 'true');
  assert.equal(t.$('#elements-tabs-tab-1').textContent, 'Personnages (9)');
});

test('a poll that finds the same lists leaves the page as it is (focus and scroll stay)', async () => {
  const t = await panel({ mode: 'rail' });
  const before = t.$('#rail-entry-elowen');
  t.shell.moved();
  await settle();
  t.shell.moved();
  await settle();
  assert.equal(t.$('#rail-entry-elowen'), before);
  const tabs = await panel({});
  const row = tabs.$('#elements-char-elowen');
  tabs.shell.moved();
  await settle();
  tabs.shell.moved();
  await settle();
  assert.equal(tabs.$('#elements-char-elowen'), row);
});

test('another campaign shows nothing of the last one while it is read, nor its search', async () => {
  const t = await panel({ mode: 'rail', campaign: 'a' });
  await t.type('elowen');
  t.mounted.update({ mode: 'rail', campaign: 'b' });
  assert.equal((t.$('#rail-search-input') as HTMLInputElement).value, '');
  assert.deepEqual(t.ids(), []);
  await settle();
  assert.equal(t.ids().length, 16);
  assert.equal(t.$('#rail-count').textContent, '');
  assert.equal(t.bus.asked(), 2);
});

test('a switch of mode starts from an empty search and asks the sheet again', async () => {
  const t = await panel({ mode: 'tabs' });
  t.mounted.update({ mode: 'rail' });
  await settle();
  await t.type('elowen');
  t.mounted.update({ mode: 'tabs' });
  t.mounted.update({ mode: 'rail' });
  await settle();
  assert.equal(t.ids().length, 16);
  assert.equal(t.bus.asked(), 3);
});

test('a panel taken off the page lets go of the sheet’s words too', async () => {
  const p = page();
  const bus = fakeBus();
  mount(p.host, { mode: 'rail' }, fakeShell().shell, bus.bus);
  await settle();
  p.host.remove();
  assert.doesNotThrow(() => bus.tell({ id: 'elowen' }));
});

test('the style is put once on the page, whatever the number of panels', async () => {
  const t = await panel({});
  const again = t.document.createElement('div');
  t.document.body.append(again);
  mount(again, { mode: 'rail' }, fakeShell().shell, fakeBus().bus);
  assert.equal(t.document.querySelectorAll('style[data-micro-ui="CampaignElements"]').length, 1);
});

// ---- the bus of the page

test('the page’s bus speaks the mock-up’s words on the document: open-entity, entity-query and entity-opened', async () => {
  const t = page();
  const heard: Array<[string, unknown]> = [];
  for (const name of ['open-entity', 'entity-query']) t.document.addEventListener(`fdm:${name}`, (e) => heard.push([name, (e as CustomEvent).detail]));
  const bus = domBus(t.document);
  bus.open({ id: 'elowen' });
  bus.open({ new: 'lieu' });
  bus.query();
  assert.deepEqual(heard, [['open-entity', { id: 'elowen' }], ['open-entity', { new: 'lieu' }], ['entity-query', {}]]);

  const told: Opened[] = [];
  const stop = bus.onOpened((o) => told.push(o));
  t.document.dispatchEvent(new t.window.CustomEvent('fdm:entity-opened', { detail: { id: 'morlox', name: 'Morlox' } }));
  t.document.dispatchEvent(new t.window.CustomEvent('fdm:entity-opened', { detail: null }));
  stop();
  t.document.dispatchEvent(new t.window.CustomEvent('fdm:entity-opened', { detail: { id: 'x' } }));
  assert.deepEqual(told, [{ id: 'morlox', name: 'Morlox' }, {}]);
});

test('with the page’s bus, a click in the rail is heard on the document as an open request', async () => {
  const t = page();
  const heard: unknown[] = [];
  t.document.addEventListener('fdm:open-entity', (e) => heard.push((e as CustomEvent).detail));
  mount(t.host, { mode: 'rail' }, fakeShell().shell);
  await settle();
  t.document.querySelector<HTMLElement>('#rail-entry-vesna')?.click();
  t.document.querySelector<HTMLElement>('#rail-new-event')?.click();
  assert.deepEqual(heard, [{ id: 'vesna' }, { new: 'evenement' }]);
  t.document.dispatchEvent(new t.window.CustomEvent('fdm:entity-opened', { detail: { id: 'vesna' } }));
  assert.equal(t.document.querySelector('#rail-entry-vesna')?.getAttribute('aria-current'), 'true');
});
