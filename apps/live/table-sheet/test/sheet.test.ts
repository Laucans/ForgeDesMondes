import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Row {
  id: string;
  description: string;
  features: unknown[];
}

const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));
const ENTITIES = fixture('entities') as { characters: Row[]; places: Row[]; events: Row[] };
const MEDIA = fixture('media.listMedia');
const ALL = [...ENTITIES.characters, ...ENTITIES.places, ...ENTITIES.events];

interface Sent {
  dataCapability: string;
  target: { aggregate: string };
  payload: Record<string, unknown>;
}

interface Options {
  /** What the server answers each command, in order; the last answer repeats. */
  status?: string | string[];
  violations?: string[];
  /** Keeps every command waiting until it is released. */
  hold?: Promise<void>;
  /** Makes the read of these capabilities throw. */
  failing?: string[];
  /** Entities by id, instead of the fixtures'. */
  entities?: unknown[];
}

/** A shell over the mock-up's fixtures: reads answer them as mock/src/reads.js does, a command is recorded and answered. */
function fakeShell(options: Options = {}) {
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables?: Record<string, unknown> }> = [];
  const known = options.entities ?? ALL;
  /** The command's answer, then each lookup's: the next of the list, the last one repeating. */
  let step = 0;
  const nextStatus = (): string => {
    step += 1;
    return (Array.isArray(options.status) ? (options.status[step - 1] ?? options.status.at(-1)) : options.status) ?? 'applied';
  };
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      if (options.failing?.includes(id)) throw new Error('the server is not there');
      if (id === 'campaign.getEntity') return { asOf: 1, data: structuredClone(known.find((e) => (e as { id: string }).id === variables?.id) ?? null) };
      if (id === 'media.listMedia') return { asOf: 1, data: structuredClone(MEDIA) };
      throw new Error(`a read the sheet does not declare: ${id}`);
    },
    async command(body) {
      sent.push(body as Sent);
      await options.hold;
      return { commandId: `c${sent.length}`, result: { status: nextStatus(), violations: options.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: nextStatus(), violations: options.violations ?? [] } };
    },
  };
  return { shell, sent, reads };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 3 };

const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function sheet(props: Record<string, unknown>, options?: Options) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="TableSheet"></div></body></html>', { url: 'http://localhost/' });
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the sheet`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true, cancelable: true }));
    await settle();
  };
  /** An element opened on the screen, as the panel does. */
  const open = async (detail: unknown): Promise<void> => {
    dom.window.document.dispatchEvent(new dom.window.CustomEvent('fdm:open', { detail }));
    await settle();
  };
  const heard: string[] = [];
  for (const name of ['fdm:open', 'fdm:open-map']) dom.window.document.addEventListener(name, (e) => heard.push(`${name} ${JSON.stringify((e as CustomEvent).detail)}`));
  return { dom, host, $, click, open, heard, update: (next: Record<string, unknown>) => mounted.update(next), ...fake };
}

const text = (element: Element): string => (element.textContent ?? '').replace(/\s+/g, ' ').trim();
/** The words of a row of cells, one space between cells. */
const cells = (element: Element): string => [...element.children].map(text).join(' ');

test('given an id as prop, the heading is the entity’s name and Modifier links to the entity editor', async () => {
  const s = await sheet({ type: 'character', id: 'morlox' });
  assert.equal(text(s.$('h2')), 'Morlox');
  assert.equal(s.$('h2').id, 'sheet-title');
  const edit = s.$('a#sheet-edit');
  assert.equal(text(edit), 'Modifier');
  assert.equal(edit.getAttribute('href'), 'fiche-entite.html?id=morlox');
  assert.deepEqual(s.reads.map((r) => r.id), ['campaign.getEntity', 'media.listMedia']);
  assert.deepEqual(s.reads[0]?.variables, { id: 'morlox' });
});

test('a character’s sheet shows the portrait, the badge, the text blocks and a stat block with CA, PV, speed and the six abilities', async () => {
  const s = await sheet({ type: 'character', id: 'morlox' });
  const morlox = ENTITIES.characters.find((c) => c.id === 'morlox') as Row;
  assert.equal(text(s.$('.kit-badge')), 'PNJ');
  assert.equal(s.$('.mf-sheet__art .art').getAttribute('role'), 'img');
  assert.match(text(s.$('#sheet-text')), new RegExp(morlox.description.slice(0, 30)));
  assert.equal(text(s.$('#sheet-behavior h3')), 'Pour l’interpréter');
  assert.equal(text(s.$('#sheet-background summary')), 'Background');
  assert.equal(text(s.$('#sheet-notes h3')), 'Notes du MJ');
  const vitals = [...s.host.querySelectorAll('#sheet-statblock .kit-statblock__vital')].map(cells);
  assert.deepEqual(vitals, ['CA 13', 'PV 18', 'Vitesse 9 m']);
  const abilities = [...s.host.querySelectorAll('#sheet-statblock .kit-statblock__ability')].map(cells);
  assert.deepEqual(abilities, ['FOR 8 −1', 'DEX 14 +2', 'CON 10 +0', 'INT 13 +1', 'SAG 9 −1', 'CHA 12 +1']);
  assert.equal(s.host.querySelectorAll('#sheet-statblock .kit-statblock__feature').length, morlox.features.length);
});

test('a player character is badged PJ; a character with no behaviour, background or notes shows no empty block', async () => {
  const elowen = { id: 'e', name: 'Elowen', kind: 'pj', description: 'Rôdeuse.', stats: {}, behavior: '', background: '', notes: '' };
  const s = await sheet({ id: 'e' }, { entities: [elowen] });
  assert.equal(text(s.$('.kit-badge')), 'PJ');
  for (const id of ['sheet-behavior', 'sheet-background', 'sheet-notes']) assert.equal(s.host.querySelector(`#${id}`), null);
  // No score is a dash, not an invented 10.
  assert.equal(cells(s.$('#sheet-statblock .kit-statblock__ability')), 'FOR — ');
});

test('a place lists the characters met there, each opening its own sheet on a click', async () => {
  const s = await sheet({ type: 'place', id: 'taverne-sanglier' });
  assert.equal(text(s.$('.kit-badge')), 'Lieu');
  assert.equal(text(s.$('#sheet-place-characters h3')), 'On y croise');
  const chips = [...s.host.querySelectorAll('#sheet-place-characters button')];
  assert.deepEqual(chips.map(text), ['Gaspard Grosmollet', 'Morlox']);
  assert.equal(s.host.querySelector('#sheet-statblock'), null);
  await s.click('button[data-open="character:morlox"]');
  assert.deepEqual(s.heard.slice(0, 1), ['fdm:open {"type":"character","id":"morlox"}']);
  assert.equal(text(s.$('h2')), 'Morlox');
  assert.equal(s.$('a#sheet-edit').getAttribute('href'), 'fiche-entite.html?id=morlox');
  assert.ok(s.$('#sheet-statblock'));
});

test('a place with nobody to meet has no “On y croise”, an event is badged and has no stat block', async () => {
  assert.equal((await sheet({ type: 'place', id: 'moulin' })).host.querySelector('#sheet-place-characters'), null);
  const s = await sheet({ type: 'event', id: 'incendie-moulin' });
  assert.equal(text(s.$('.kit-badge')), 'Événement');
  assert.equal(text(s.$('h2')), 'L’incendie du moulin');
  assert.equal(s.host.querySelector('#sheet-statblock'), null);
});

test('a character of a place that has no sheet is left out of “On y croise”', async () => {
  const place = { id: 'p', name: 'Pont', kind: 'lieu', description: 'Un pont.', characters: ['morlox', 'ghost'] };
  const s = await sheet({ id: 'p' }, { entities: [place, ...ENTITIES.characters] });
  assert.deepEqual([...s.host.querySelectorAll('#sheet-place-characters button')].map(text), ['Morlox']);
});

test('an element with an illustration offers both sends; the scene send is live.showOnPlayerView with that media as base', async () => {
  const s = await sheet({ type: 'character', id: 'gaspard', campaign: 'c-1' });
  assert.equal(text(s.$('#sheet-send-scene')), 'Envoyer l’illustration — scène');
  assert.equal(text(s.$('#sheet-send-window')), 'Envoyer — fenêtre');
  assert.equal(s.$('#sheet-send-scene').dataset.writes, 'live.showOnPlayerView');
  assert.equal(s.$('#sheet-send-window').dataset.writes, 'live.showWindowOnPlayerView');
  await s.click('#sheet-send-scene');
  assert.equal(s.sent.length, 1);
  assert.deepEqual(s.sent[0], {
    dataCapability: 'live.showOnPlayerView@1',
    target: { aggregate: 'Diffusion' },
    payload: { campaign: 'c-1', kind: 'illustration', media: 'ill-gaspard' },
    caller: { by: 'dm', from: 'TableSheet', roles: [] },
  });
  assert.match(text(s.$('#sheet-status')), /Gaspard, le tavernier.*envoyée aux joueurs, en mode scène/);
});

test('the window send is live.showWindowOnPlayerView, and only that', async () => {
  const s = await sheet({ type: 'character', id: 'gaspard', campaign: 'c-1' });
  await s.click('#sheet-send-window');
  assert.equal(s.sent.length, 1);
  assert.equal(s.sent[0]?.dataCapability, 'live.showWindowOnPlayerView@1');
  assert.deepEqual(s.sent[0]?.payload, { campaign: 'c-1', media: 'ill-gaspard' });
  assert.match(text(s.$('#sheet-status')), /en mode fenêtre/);
});

test('a place sends the illustration the library attaches to it', async () => {
  const s = await sheet({ type: 'place', id: 'taverne-sanglier', campaign: 'c-1' });
  await s.click('#sheet-send-scene');
  assert.equal(s.sent[0]?.payload.media, 'ill-salle-sanglier');
  assert.deepEqual(s.reads.filter((r) => r.id === 'media.listMedia').map((r) => r.variables), [{ campaign: 'c-1' }]);
});

test('an element with no illustration offers no send button', async () => {
  for (const [type, id] of [['character', 'morlox'], ['place', 'tour-ebene'], ['event', 'incendie-moulin']]) {
    const s = await sheet({ type, id });
    assert.equal(s.host.querySelector('[data-writes]'), null, `${id} offers no send`);
    assert.equal(s.host.querySelector('#sheet-send-scene'), null);
  }
});

test('the sheet is read-only: no field, no editable part, no write but the two sends', async () => {
  for (const id of ['morlox', 'taverne-sanglier', 'gaspard']) {
    const s = await sheet({ id });
    assert.equal(s.host.querySelectorAll('input, textarea, select, [contenteditable]').length, 0, `${id} has no field`);
    const writes = [...new Set([...s.host.querySelectorAll('[data-writes]')].map((p) => p.getAttribute('data-writes')))].sort();
    assert.ok(writes.every((w) => w === 'live.showOnPlayerView' || w === 'live.showWindowOnPlayerView'));
  }
});

test('the parts name the unit they show or trigger, by identifier', async () => {
  const s = await sheet({ type: 'character', id: 'gaspard' });
  const named = (attribute: string): string[] => [...new Set([...s.host.querySelectorAll(`[${attribute}]`)].map((p) => p.getAttribute(attribute) ?? ''))].sort();
  assert.deepEqual(named('data-reads'), ['campaign.getEntity', 'media.listMedia']);
  assert.deepEqual(named('data-writes'), ['live.showOnPlayerView', 'live.showWindowOnPlayerView']);
});

test('a window needs its campaign: without one it is not sent and the DM is told; the scene send leaves it to the media', async () => {
  const s = await sheet({ type: 'character', id: 'gaspard' });
  await s.click('#sheet-send-window');
  assert.equal(s.sent.length, 0);
  assert.match(text(s.$('#sheet-status')), /campagne n’est pas connue/);
  await s.click('#sheet-send-scene');
  assert.deepEqual(s.sent[0]?.payload, { kind: 'illustration', media: 'ill-gaspard' });
  assert.deepEqual(s.reads.filter((r) => r.id === 'media.listMedia').map((r) => r.variables), [{}]);
});

test('a campaign the entity’s answer names is the one a window is sent to', async () => {
  const gaspard = { ...ENTITIES.characters.find((c) => c.id === 'gaspard'), campaignId: 'c-7' };
  const s = await sheet({ id: 'gaspard' }, { entities: [gaspard] });
  await s.click('#sheet-send-window');
  assert.equal(s.sent[0]?.payload.campaign, 'c-7');
  assert.deepEqual(s.reads.find((r) => r.id === 'media.listMedia')?.variables, { campaign: 'c-7' });
});

test('a refused send says why; one still pending says where it stands; the buttons are free again after', async () => {
  const refused = await sheet({ id: 'gaspard', campaign: 'c-1' }, { status: 'rejected', violations: ['window-proportional'] });
  await refused.click('#sheet-send-window');
  assert.equal(text(refused.$('#sheet-status')), 'Envoi refusé : la fenêtre doit garder les proportions de l’image.');
  assert.equal((refused.$('#sheet-send-window') as HTMLButtonElement).disabled, false);
  const waiting = await sheet({ id: 'gaspard', campaign: 'c-1' }, { status: 'awaiting_review' });
  await waiting.click('#sheet-send-scene');
  assert.match(text(waiting.$('#sheet-status')), /n’a pas abouti \(état : awaiting_review\)/);
});

test('a command that follows a queue is asked about until it settles', async () => {
  const s = await sheet({ id: 'gaspard', campaign: 'c-1' }, { status: ['queued', 'queued', 'applied'] });
  await s.click('#sheet-send-scene');
  assert.match(text(s.$('#sheet-status')), /envoyée aux joueurs/);
});

test('while a send is under way the buttons are held, and a second click sends nothing', async () => {
  let release: () => void = () => undefined;
  const hold = new Promise<void>((resolve) => (release = resolve));
  const s = await sheet({ id: 'gaspard', campaign: 'c-1' }, { hold });
  await s.click('#sheet-send-scene');
  assert.equal((s.$('#sheet-send-scene') as HTMLButtonElement).disabled, true);
  assert.equal((s.$('#sheet-send-window') as HTMLButtonElement).disabled, true);
  s.$('#sheet-send-scene').dispatchEvent(new s.dom.window.MouseEvent('click', { bubbles: true }));
  await s.click('#sheet-send-window');
  assert.equal(s.sent.length, 1);
  release();
  await settle();
  assert.equal((s.$('#sheet-send-scene') as HTMLButtonElement).disabled, false);
});

test('a send that cannot reach the server says so', async () => {
  const s = await sheet({ id: 'gaspard', campaign: 'c-1' });
  s.shell.command = async () => {
    throw new Error('offline');
  };
  await s.click('#sheet-send-scene');
  assert.equal(text(s.$('#sheet-status')), 'La commande n’a pas pu être envoyée.');
});

test('an element opened on the screen replaces the sheet; props that change do too', async () => {
  const s = await sheet({ type: 'place', id: 'taverne-sanglier' });
  await s.open({ type: 'event', id: 'incendie-moulin' });
  assert.equal(text(s.$('h2')), 'L’incendie du moulin');
  await s.open({ type: 'character', id: 'elowen' });
  assert.equal(text(s.$('h2')), 'Elowen Ventargent');
  assert.equal(text(s.$('.kit-badge')), 'PJ');
  s.update({ type: 'place', id: 'valombre' });
  await settle();
  assert.equal(text(s.$('h2')), 'Valombre');
  // Props the screen resolves to the same element leave the sheet alone, and an event with no id changes nothing.
  const before = s.reads.length;
  s.update({ type: 'place', id: 'valombre' });
  await s.open({ type: 'place' });
  await s.open(null);
  assert.equal(s.reads.length, before);
});

test('the answer to an earlier element never replaces the one opened since', async () => {
  const s = await sheet({ id: 'morlox' });
  const slow = new Promise<void>((resolve) => setTimeout(resolve, 5));
  const read = s.shell.capability.bind(s.shell);
  s.shell.capability = async (id, sha, variables) => {
    if (variables?.id === 'taverne-sanglier') await slow;
    return read(id, sha, variables);
  };
  await s.open({ type: 'place', id: 'taverne-sanglier' });
  await s.open({ type: 'event', id: 'incendie-moulin' });
  await new Promise((resolve) => setTimeout(resolve, 20));
  await settle();
  assert.equal(text(s.$('h2')), 'L’incendie du moulin');
});

test('no element opened says so, and reads nothing', async () => {
  const s = await sheet({});
  assert.equal(text(s.$('#sheet-empty')), 'Aucun élément ouvert.');
  assert.equal(s.reads.length, 0);
  await s.open({ type: 'character', id: 'morlox' });
  assert.equal(text(s.$('h2')), 'Morlox');
});

test('an unknown element and an unreadable sheet are said, not drawn', async () => {
  const unknown = await sheet({ id: 'nobody' });
  assert.equal(text(unknown.$('#sheet-missing')), 'Élément introuvable.');
  assert.equal(unknown.host.querySelector('[data-writes]'), null);
  const down = await sheet({ id: 'morlox' }, { failing: ['campaign.getEntity'] });
  assert.equal(text(down.$('#sheet-error')), 'Impossible de lire la fiche.');
});

test('a library that cannot be read leaves the sheet without its illustration', async () => {
  const s = await sheet({ id: 'gaspard' }, { failing: ['media.listMedia'] });
  assert.equal(text(s.$('h2')), 'Gaspard Grosmollet');
  assert.equal(s.host.querySelector('[data-writes]'), null);
});

test('an illustration opened as an element shows its file and sends itself, with no editor', async () => {
  const s = await sheet({ type: 'illustration', id: 'ill-vesna', campaign: 'c-1' });
  assert.equal(text(s.$('h2')), 'Vesna la Balafrée');
  assert.equal(text(s.$('.kit-badge')), 'Illustration');
  assert.equal(text(s.$('#sheet-text')), 'vesna-balafree.png · 940 Ko');
  assert.equal(s.host.querySelector('#sheet-edit'), null);
  assert.deepEqual(s.reads.map((r) => r.id), ['media.listMedia']);
  await s.click('#sheet-send-scene');
  assert.equal(s.sent[0]?.payload.media, 'ill-vesna');
  assert.equal((await sheet({ type: 'illustration', id: 'ill-gone' })).host.querySelector('#sheet-missing') !== null, true);
});

test('Placer sur la carte raises the open-map event for the character', async () => {
  const s = await sheet({ type: 'character', id: 'morlox' });
  await s.click('#sheet-to-map');
  assert.deepEqual(s.heard, ['fdm:open-map {"place":"morlox"}']);
  assert.equal((await sheet({ type: 'place', id: 'moulin' })).host.querySelector('#sheet-to-map'), null);
});

test('everything the answers hold is escaped', async () => {
  const hostile = { id: 'x"><i>', name: '<script>1</script>', kind: 'pnj', aliases: ['<b>a</b>'], description: '<img src=x>', behavior: '<u>b</u>', notes: '&n', features: [{ name: '<f>', text: '<t>' }] };
  const s = await sheet({ id: 'x"><i>' }, { entities: [hostile] });
  assert.equal(s.host.querySelector('script, img, u, b, i, f, t'), null);
  assert.equal(text(s.$('h2')), '<script>1</script>');
  assert.equal(s.$('a#sheet-edit').getAttribute('href'), 'fiche-entite.html?id=x%22%3E%3Ci%3E');
});

test('the style is added once, whatever the number of sheets on the page', async () => {
  const s = await sheet({ id: 'morlox' });
  mount(s.host, { id: 'morlox' }, s.shell, timing);
  assert.equal(s.dom.window.document.querySelectorAll('style[data-micro-ui="TableSheet"]').length, 1);
  assert.doesNotMatch(s.dom.window.document.querySelector('style[data-micro-ui="TableSheet"]')?.textContent ?? '', /#[0-9a-fA-F]{3,8}\b|rgba?\(/);
});
