import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { center } from '../src/hexgrid.ts';
import { mount, type Timing } from '../src/index.ts';
import { placeStage } from '../src/model.ts';
import type { Shell } from '../src/types.ts';
import { assertDeclared } from './manifests.ts';

const fixtureOf = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));

interface Raw {
  id: string;
  state: { tokens: Array<Record<string, unknown>>; combat: null };
  [key: string]: unknown;
}
interface Sent {
  dataCapability: string;
  target: { aggregate: string; id?: string };
  payload: Record<string, unknown>;
  caller: { by: string; from: string; roles: string[] };
}

const map = (() => {
  const fx = fixtureOf('media.getBattlemap') as { grid: { opacity: number } };
  // HexGrid@1: the opacity is a fraction of 1 (the mock-up's fixture says percent).
  return { ...fx, grid: { ...fx.grid, opacity: fx.grid.opacity / 100 } } as { id: string; width: number; height: number; grid: { size: number; ox: number; oy: number; orient: 'pointy'; color: string; opacity: number } };
})();

/** A shell over a small table: it answers the three reads and applies the commands as the table would. */
function fakeShell(options: { diffusion?: unknown; tokens?: (all: Array<Record<string, unknown>>) => Array<Record<string, unknown>>; reject?: string; leaky?: boolean; omitId?: boolean } = {}) {
  const instance = structuredClone(fixtureOf('table.getBattlemapInstance')) as Raw;
  if (options.tokens) instance.state.tokens = options.tokens(instance.state.tokens);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables: Record<string, unknown> | undefined }> = [];
  const watchers: Array<(version: number) => void> = [];
  const diffused = { value: options.diffusion ?? { base: { kind: 'cover', id: 'ill-cover' }, window: null, view: null } };
  const find = (id: unknown) => instance.state.tokens.find((t) => t.id === id) as Record<string, unknown>;
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, variables });
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      if (id === 'media.getBattlemap') return { asOf: 1, data: map };
      if (id === 'campaign.listCharacters') return { asOf: 1, data: fixtureOf('campaign.listCharacters') };
      if (id === 'live.getDiffusion') return { asOf: 1, data: diffused.value };
      assert.equal(id, 'table.getBattlemapInstance');
      const own = structuredClone(instance);
      if (options.omitId) delete (own as Partial<Raw>).id;
      if (variables?.view === 'player' && !options.leaky) own.state.tokens = own.state.tokens.filter((t) => t.visible).map(({ hp, hpMax, ...t }) => ({ ...t, healthBand: t.kind === 'pnj' && hp === hpMax ? 'indemne' : null }));
      return { asOf: 1, data: own };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      assertDeclared(command);
      if (options.reject) return { commandId: `c${sent.length}`, result: { status: 'rejected', violations: [options.reject] } };
      const { payload } = command;
      switch (command.dataCapability) {
        case 'table.moveToken@1':
          Object.assign(find(payload.token), { q: payload.q, r: payload.r });
          break;
        case 'table.renameToken@1':
          find(payload.token).name = payload.name;
          break;
        case 'table.setTokenVisibility@1':
          find(payload.token).visible = payload.visible;
          break;
        case 'table.setTokenCondition@1':
          {
            const token = find(payload.token);
            const held = token.states as string[];
            token.states = payload.on ? [...held.filter((s) => s !== payload.state), String(payload.state)] : held.filter((s) => s !== payload.state);
          }
          break;
        case 'table.removeToken@1':
          instance.state.tokens = instance.state.tokens.filter((t) => t.id !== payload.token);
          break;
        case 'table.placeTokens@1': {
          // multi-drop-numbering, as the table does it.
          const taken = Math.max(0, ...instance.state.tokens.filter((t) => t.char === payload.character).map((t) => Number(t.num ?? 0)));
          for (let i = 1; i <= Number(payload.count); i += 1) {
            instance.state.tokens.push({ id: `t-new-${taken + i}`, char: payload.character, kind: 'pnj', name: `Bandit ${taken + i}`, num: taken + i, q: Number(payload.q) + i - 1, r: payload.r, visible: true, hp: 11, hpMax: 11, states: [], dead: false });
          }
          break;
        }
        default:
      }
      return { commandId: `c${sent.length}`, result: { status: 'applied', violations: [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: 'applied', violations: [] } };
    },
    onDataVersion(listener) {
      watchers.push(listener);
      // As the shell does: the first answer says where the data stands.
      listener(0);
      return () => undefined;
    },
  };
  /** The data moves: every watching view reads again. */
  const moved = (): void => watchers.forEach((listener, version) => listener(version));
  return { shell, sent, reads, instance, diffused, moved };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5, watchMs: 1000, frameMs: 0 };
const flush = async (): Promise<void> => {
  for (let i = 0; i < 12; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

/** A view as the DM's (705 × 470, the map's ratio) and as the projected one (984 × 656). */
const DM_VIEW = { width: 705, height: 470 };
const PLAYER_VIEW = { width: 984, height: 656 };

async function battlemap(props: Record<string, unknown>, options?: Parameters<typeof fakeShell>[0], url = 'http://localhost/', size?: { width: number; height: number }) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>', { url });
  const { document } = dom.window;
  // jsdom lays nothing out: the map's view is the size the test gives it, and a resize is the test's to announce.
  const laid = { ...(size ?? { width: 0, height: 0 }) };
  const observers: Array<() => void> = [];
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientWidth', { configurable: true, get(this: HTMLElement) { return this.id === 'battlemap-view' ? laid.width : 0; } });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientHeight', { configurable: true, get(this: HTMLElement) { return this.id === 'battlemap-view' ? laid.height : 0; } });
  Object.defineProperty(dom.window, 'ResizeObserver', { configurable: true, value: class { constructor(listener: () => void) { observers.push(listener); } observe() {} disconnect() {} } });
  const resize = (next: { width: number; height: number }) => {
    Object.assign(laid, next);
    observers.forEach((listener) => listener());
  };
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, { instance: 'bmi-gue-s4', ...props }, fake.shell, timing);
  await flush();
  const pointer = (type: string, target: Element, at: { x: number; y: number }) =>
    target.dispatchEvent(new dom.window.MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: at.x, clientY: at.y }));
  const at = (q: number, r: number) => center(map.grid, { q, r });
  const token = (id: string) => document.querySelector<HTMLElement>(`#token-${id}`) as HTMLElement;
  const click = async (selector: string) => {
    (document.querySelector(selector) as HTMLElement).click();
    await flush();
  };
  return { dom, document, host, fake, pointer, at, token, click, resize };
}

const left = (el: HTMLElement) => el.style.left;
const px = (n: number) => `${n.toFixed(1)}px`;

test('the DM view draws the map, every token — a hidden one marked — and names its parts', async () => {
  const { document, fake } = await battlemap({ view: 'dm' });
  assert.deepEqual(fake.reads.map((r) => r.id).sort(), ['campaign.listCharacters', 'media.getBattlemap', 'table.getBattlemapInstance']);
  assert.deepEqual(fake.reads.find((r) => r.id === 'table.getBattlemapInstance')?.variables, { instance: 'bmi-gue-s4', view: 'dm' });
  assert.deepEqual(fake.reads.find((r) => r.id === 'media.getBattlemap')?.variables, { id: 'bm-gue' });
  assert.equal(document.querySelectorAll('.kit-token__item').length, 8);
  assert.ok(document.querySelector('#token-t-vesna-1.kit-token__item--hidden'), 'the hidden ambusher is marked for the DM');
  assert.equal(document.querySelector('#battlemap-title')?.textContent, 'Le Gué des Corbeaux');
  assert.match(document.querySelector('.bm-title .small')?.textContent ?? '', /8 tokens · 1 caché · 1 hex = 5 pi/);
  assert.match(document.querySelector('#token-t-bandit-1')?.textContent ?? '', /11\/11 PV/);
  for (const unit of ['table.getBattlemapInstance', 'media.getBattlemap', 'campaign.listCharacters']) assert.ok(document.querySelector(`[data-reads~="${unit}"]`), `a part reads ${unit}`);
  for (const unit of ['table.moveToken', 'live.showOnPlayerView']) assert.ok(document.querySelector(`[data-writes="${unit}"]`), `a part writes ${unit}`);
});

test('a token dragged 3 hexes away reads « 3 hex · 15 pi » under the cursor and the drop moves it', async () => {
  const { document, fake, pointer, at, token } = await battlemap({ view: 'dm' });
  const from = at(0, 6);
  const to = at(3, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 20, y: from.y });
  pointer('pointermove', token('t-elowen'), to);
  const tip = document.querySelector<HTMLElement>('#battlemap-tip');
  assert.equal(tip?.hidden, false);
  assert.equal(tip?.textContent, '3 hex · 15 pi');
  assert.ok(token('t-elowen').classList.contains('kit-token__item--carried'));
  assert.equal(document.querySelectorAll('#battlemap-over polygon').length, 4, 'every hex of the path is drawn');
  pointer('pointerup', token('t-elowen'), to);
  await flush();
  assert.equal(tip?.hidden, true, 'the counter goes with the drag');
  assert.equal(fake.sent.length, 1);
  assert.deepEqual(fake.sent[0], {
    dataCapability: 'table.moveToken@1',
    target: { aggregate: 'BattlemapInstance', id: 'bmi-gue-s4' },
    payload: { token: 't-elowen', q: 3, r: 6 },
    caller: { by: 'dm', from: 'Battlemap', roles: ['role:approver'] },
  });
  assert.equal(left(token('t-elowen')), px(to.x), 'the map is read again: the token stands where the table keeps it');
  assert.equal(document.querySelectorAll('#battlemap-over polygon').length, 0);
});

test('a drop on the same hex, or a press without a drag, sends no move', async () => {
  const { fake, pointer, at, token, document } = await battlemap({ view: 'dm' });
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), from);
  pointer('pointerup', token('t-elowen'), from);
  await flush();
  pointer('pointerdown', token('t-kael'), at(1, 6));
  pointer('pointerup', token('t-kael'), at(1, 6));
  await flush();
  assert.equal(fake.sent.length, 0);
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), false, 'a press opens the token’s menu');
  assert.equal(document.querySelector('#inspector-title')?.textContent, 'Kael');
});

test('a move the table refuses puts the token back and says why', async () => {
  const { document, pointer, at, token } = await battlemap({ view: 'dm' }, { reject: 'instance-isolated' });
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), at(2, 6));
  pointer('pointerup', token('t-elowen'), at(2, 6));
  await flush();
  assert.equal(left(token('t-elowen')), px(from.x));
  const status = document.querySelector<HTMLElement>('#battlemap-status');
  assert.equal(status?.hidden, false);
  assert.match(status?.textContent ?? '', /Le déplacement refusé : ce token n’est pas sur cette carte/);
});

test('a character dropped « × 3 » on tokens 1 and 2 is placed 3 times and the map shows 3, 4, 5', async () => {
  const { document, dom, fake, at, click } = await battlemap({ view: 'dm' }, { tokens: (all) => all.filter((t) => t.id !== 't-bandit-3') });
  const view = document.querySelector('#battlemap-view') as HTMLElement;
  const drop = at(5, 5);
  const event = new dom.window.Event('drop', { bubbles: true, cancelable: true });
  Object.assign(event, { clientX: drop.x, clientY: drop.y, dataTransfer: { getData: (type: string) => (type === 'text/fdm-character' ? 'bandit' : '') } });
  view.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true);
  const count = document.querySelector<HTMLInputElement>('#battlemap-drop-count') as HTMLInputElement;
  assert.equal(document.querySelector('#battlemap-drop-numbers')?.textContent, 'Numérotée Bandit 3, renommable');
  count.value = '3';
  count.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  assert.equal(document.querySelector('#battlemap-drop-numbers')?.textContent, 'Numérotées de Bandit 3 à Bandit 5, renommables');
  assert.equal(fake.sent.length, 0, 'nothing is sent before the drop is confirmed');
  await click('#battlemap-drop-ok');
  assert.equal(document.querySelector('#battlemap-drop'), null);
  assert.equal(fake.sent.length, 1);
  assert.equal(fake.sent[0]?.dataCapability, 'table.placeTokens@1');
  assert.deepEqual(fake.sent[0]?.payload, { character: 'bandit', q: 5, r: 5, count: 3 });
  const nums = [...document.querySelectorAll('.kit-token__item')].filter((el) => /^Bandit/.test(el.querySelector('.kit-token__name')?.textContent ?? '')).map((el) => el.querySelector('.kit-token__num')?.textContent);
  assert.deepEqual(nums, ['1', '2', '3', '4', '5']);
});

test('a cancelled drop sends nothing, and a PC dropped again is moved, not placed twice', async () => {
  const { document, dom, fake, at, click } = await battlemap({ view: 'dm' });
  const dropOf = async (id: string, where: { x: number; y: number }) => {
    const event = new dom.window.Event('drop', { bubbles: true, cancelable: true });
    Object.assign(event, { clientX: where.x, clientY: where.y, dataTransfer: { getData: () => id } });
    (document.querySelector('#battlemap-view') as HTMLElement).dispatchEvent(event);
    await flush();
  };
  await dropOf('morlox', at(4, 4));
  await click('#battlemap-drop-cancel');
  assert.equal(document.querySelector('#battlemap-drop'), null);
  assert.equal(fake.sent.length, 0);
  assert.match(document.querySelector('#battlemap-drop')?.textContent ?? 'none', /none/);
  await dropOf('elowen', at(2, 2));
  assert.match(document.querySelector('#battlemap-drop')?.textContent ?? '', /Un PJ n’a qu’un token/);
  await click('#battlemap-drop-ok');
  assert.equal(fake.sent[0]?.dataCapability, 'table.moveToken@1');
  assert.deepEqual(fake.sent[0]?.payload, { token: 't-elowen', q: 2, r: 2 });
});

test('the token menu hides, renames, marks and removes — each its own command, the map read again', async () => {
  const { document, dom, fake, pointer, at, token, click } = await battlemap({ view: 'dm' });
  pointer('pointerdown', token('t-bandit-2'), at(10, 6));
  pointer('pointerup', token('t-bandit-2'), at(10, 6));
  assert.equal(document.querySelector('#inspector-title')?.textContent, 'Bandit 2');
  assert.equal(document.querySelectorAll('#inspector-instances .chip').length, 3, 'the other instances of the character are one click away');
  assert.equal(document.querySelectorAll('.kit-token__item').length, 8);

  await click('#inspector-visible');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2', visible: false });
  assert.equal(fake.sent.at(-1)?.dataCapability, 'table.setTokenVisibility@1');
  assert.ok(token('t-bandit-2').classList.contains('kit-token__item--hidden'));
  assert.equal(document.querySelector('#inspector-visible')?.getAttribute('aria-checked'), 'false');

  await click('[data-state="À terre"]');
  assert.equal(fake.sent.at(-1)?.dataCapability, 'table.setTokenCondition@1');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2', state: 'À terre', on: true });
  assert.equal(token('t-bandit-2').querySelectorAll('.kit-token__badge').length, 1);
  await click('[data-state="Effrayé"]');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2', state: 'Effrayé', on: true });
  await click('[data-state="À terre"]');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2', state: 'À terre', on: false });

  const name = document.querySelector<HTMLInputElement>('#inspector-name') as HTMLInputElement;
  name.value = '  Bandit blessé ';
  name.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  await flush();
  assert.equal(fake.sent.at(-1)?.dataCapability, 'table.renameToken@1');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2', name: 'Bandit blessé' });
  assert.equal(token('t-bandit-2').querySelector('.kit-token__name')?.textContent, 'Bandit blessé');
  assert.equal(token('t-bandit-1').querySelector('.kit-token__name')?.textContent, 'Bandit 1', 'the others keep their names');

  const before = fake.sent.length;
  const blank = document.querySelector<HTMLInputElement>('#inspector-name') as HTMLInputElement;
  blank.value = '   ';
  blank.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  await flush();
  assert.equal(fake.sent.length, before, 'an empty name is not sent');
  assert.equal(blank.value, 'Bandit blessé');

  await click('#inspector-remove');
  assert.equal(fake.sent.at(-1)?.dataCapability, 'table.removeToken@1');
  assert.deepEqual(fake.sent.at(-1)?.payload, { token: 't-bandit-2' });
  assert.equal(document.querySelector('#token-t-bandit-2'), null);
  assert.equal(document.querySelectorAll('.kit-token__item').length, 7);
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), true);
});

test('« Envoyer aux joueurs » sends the instance with the zoom and pan as framed', async () => {
  const { document, fake, click } = await battlemap({ view: 'dm' }, undefined, undefined, DM_VIEW);
  await click('#bm-send');
  assert.deepEqual(fake.sent[0], {
    dataCapability: 'live.showOnPlayerView@1',
    target: { aggregate: 'Diffusion', id: 's4' },
    payload: { kind: 'battlemap', instance: 'bmi-gue-s4', zoom: 1, panX: 0, panY: 0 },
    caller: { by: 'dm', from: 'Battlemap', roles: ['role:approver'] },
  });
  assert.match(document.querySelector('#battlemap-status')?.textContent ?? '', /Carte envoyée aux joueurs/);
  await click('[data-act="in"]');
  await click('[data-act="in"]');
  await click('#bm-send');
  assert.equal(fake.sent[1]?.payload.zoom, 1.563);
});

test('a send the table refuses is told, not shown as done', async () => {
  const { document, click } = await battlemap({ view: 'dm' }, { reject: 'base-coherent' });
  await click('#bm-send');
  assert.match(document.querySelector('#battlemap-status')?.textContent ?? '', /L’envoi aux joueurs est refusé : cette carte n’est pas celle de la séance/);
  assert.ok(document.querySelector('#battlemap-status.is-error'));
});

test('the player view shows no hidden token, no figure and no note, an NPC only its band', async () => {
  const { document, fake, pointer, at, token } = await battlemap({ view: 'player' });
  assert.deepEqual(fake.reads.find((r) => r.id === 'table.getBattlemapInstance')?.variables, { instance: 'bmi-gue-s4', view: 'player' });
  assert.ok(!fake.reads.some((r) => r.id === 'campaign.listCharacters'), 'the players’ view lists no characters');
  assert.equal(document.querySelectorAll('.kit-token__item').length, 7);
  assert.equal(document.querySelector('#token-t-vesna-1'), null);
  assert.doesNotMatch(document.body.textContent ?? '', /\d+\/\d+ PV|\bPV\b/);
  assert.equal(token('t-bandit-1').querySelector('.kit-token__hp-label')?.textContent, 'Indemne');
  assert.equal(token('t-elowen').querySelector('.kit-token__hp'), null, 'a PC has no band');
  for (const part of ['#battlemap-toolbar .kit-button', '#token-inspector .panel__head', '.hm-bar']) assert.equal(document.querySelector(part), null, part);
  pointer('pointerdown', token('t-elowen'), at(0, 6));
  pointer('pointerup', token('t-elowen'), at(0, 6));
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), true);
  assert.equal(fake.sent.length, 0);
});

test('whatever a leaky answer carries, the player view still shows no hidden token and no figure', async () => {
  const { document, token } = await battlemap({ view: 'player' }, { leaky: true });
  assert.equal(document.querySelector('#token-t-vesna-1'), null, 'the hidden token is not drawn');
  assert.equal(document.querySelectorAll('.kit-token__item').length, 7);
  assert.doesNotMatch(document.body.textContent ?? '', /\d+\/\d+ PV|\bPV\b/);
  assert.equal(token('t-bandit-1').querySelector('.kit-token__hp'), null, 'with no band to show, nothing is');
  assert.doesNotMatch(document.body.innerHTML, /hpMax|"hp"/);
});

test('the DM driving the players’ tab moves tokens there, but sees no menu', async () => {
  const { document, fake, pointer, at, token } = await battlemap({ view: 'player', pilot: true, fill: true });
  assert.ok(document.querySelector('.bm--fill'));
  assert.ok(!document.querySelector('.bm--readonly'));
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), at(1, 7));
  pointer('pointerup', token('t-elowen'), at(1, 7));
  await flush();
  assert.equal(fake.sent[0]?.dataCapability, 'table.moveToken@1');
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), true);
});

// AC2: the DM opening player-view?pilote=1 gets 'Zoomer', and a token dragged there moves
test('player-view?pilote=1 is the DM’s pilot tab without the prop: the zoom bar and the moving tokens', async () => {
  const { document, fake, pointer, at, token } = await battlemap({ view: 'player', fill: true }, undefined, 'http://localhost/screens/player-view?pilote=1');
  assert.ok(document.querySelector('.hm-bar [aria-label="Zoomer"]'));
  assert.ok(!document.querySelector('.bm--readonly'));
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), at(1, 7));
  pointer('pointerup', token('t-elowen'), at(1, 7));
  await flush();
  assert.equal(fake.sent[0]?.dataCapability, 'table.moveToken@1');
});

// AC2: getByRole('button', {name: 'Zoomer'}) resolves to the zoom-in button alone
test('the pilot tab’s zoom-out button is not named « Zoomer »: only the zoom-in button is', async () => {
  const { document } = await battlemap({ view: 'player', pilot: true, fill: true });
  const names = Array.from(document.querySelectorAll('.hm-bar button')).map((b) => b.getAttribute('aria-label') ?? b.textContent ?? '');
  assert.deepEqual(names, ['Réduire le zoom', 'Ajuster', 'Zoomer']);
  // Playwright matches a name as a case-insensitive substring.
  assert.equal(names.filter((n) => n.toLowerCase().includes('zoomer')).length, 1);
});

// AC3: player-view with no ?pilote=1 stays read-only with no zoom bar
test('player-view with no ?pilote=1, or another value, stays read-only with no zoom bar', async () => {
  for (const url of ['http://localhost/screens/player-view', 'http://localhost/screens/player-view?pilote=0', 'http://localhost/screens/player-view?pilote=true']) {
    const { document, fake, pointer, at, token } = await battlemap({ view: 'player' }, undefined, url);
    assert.ok(document.querySelector('.bm--readonly'), url);
    assert.equal(document.querySelector('.hm-bar'), null, url);
    assert.equal(document.querySelector('[aria-label="Zoomer"]'), null, url);
    pointer('pointerdown', token('t-elowen'), at(0, 6));
    pointer('pointermove', token('t-elowen'), at(1, 7));
    pointer('pointerup', token('t-elowen'), at(1, 7));
    await flush();
    assert.equal(fake.sent.length, 0, url);
  }
});

test('new props make the Micro-UI follow another instance or view', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell();
  const mounted = mount(host, { instance: 'bmi-gue-s4', view: 'dm' }, fake.shell, timing);
  await flush();
  assert.equal(host.querySelectorAll('.kit-token__item').length, 8);
  mounted.update({ instance: 'bmi-gue-s4', view: 'dm' });
  await flush();
  const reads = fake.reads.length;
  mounted.update({ instance: 'bmi-gue-s4', view: 'player' });
  await flush();
  assert.ok(fake.reads.length > reads);
  assert.equal(host.querySelectorAll('.kit-token__item').length, 7);
});

test('the prep view lists the scene’s characters to drop', async () => {
  const { document, dom, fake } = await battlemap({ view: 'prep' });
  const chips = [...document.querySelectorAll('#battlemap-roster-list [data-char]')].map((c) => c.getAttribute('data-char'));
  assert.deepEqual(chips, ['elowen', 'brannoc', 'sirielle', 'kael', 'bandit', 'vesna', 'morlox']);
  assert.ok(document.querySelector('#battlemap-roster-list [data-writes="table.placeTokens"]'));
  (document.querySelector('#roster-bandit') as HTMLElement).click();
  assert.match(document.querySelector('#battlemap-drop')?.textContent ?? '', /Placer Bandit/);
  assert.equal(fake.sent.length, 0);
  assert.ok(dom.window.document.querySelector('#battlemap-drop-count'));
});

test('the prep view keeps the roster column from the first paint, so the map is fitted to the width it keeps', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  mount(host, { instance: 'bmi-gue-s4', view: 'prep' }, fakeShell().shell, timing);
  // Nothing is read yet: the column is already there, empty, and the inspector is an overlay of the map.
  assert.equal(host.querySelector<HTMLElement>('#battlemap-roster')?.hidden, false);
  assert.equal(host.querySelector('#battlemap-roster')?.children.length, 0);
  assert.ok(host.querySelector('.bm.bm--prep'));
  assert.match(dom.window.document.head.textContent ?? '', /\.bm #token-inspector \{ position: absolute;/);
  await flush();
  assert.equal(host.querySelector<HTMLElement>('#battlemap-roster')?.hidden, false);
  assert.ok(host.querySelector('#battlemap-roster-list'));
});

test('the dm and player views have no roster column, before or after the read', async () => {
  for (const view of ['dm', 'player']) {
    const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
    const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
    mount(host, { instance: 'bmi-gue-s4', view }, fakeShell().shell, timing);
    assert.equal(host.querySelector<HTMLElement>('#battlemap-roster')?.hidden, true, `${view} before the read`);
    assert.equal(host.querySelector('.bm--prep'), null);
    await flush();
    assert.equal(host.querySelector<HTMLElement>('#battlemap-roster')?.hidden, true, `${view} after the read`);
  }
});

test('placing from the roster is sent as the DM approving their own act, so the position is stored', async () => {
  const { document, fake } = await battlemap({ view: 'prep' });
  (document.querySelector('#roster-bandit') as HTMLElement).click();
  (document.querySelector('#battlemap-drop-ok') as HTMLElement).click();
  await flush();
  assert.equal(fake.sent.length, 1);
  assert.equal(fake.sent[0]?.dataCapability, 'table.placeTokens@1');
  assert.deepEqual(fake.sent[0]?.caller, { by: 'dm', from: 'Battlemap', roles: ['role:approver'] });
});

test('a battlemap that cannot be read is told', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const shell: Shell = {
    capability: async () => {
      throw new Error('down');
    },
    command: async () => ({}),
    commandResult: async () => ({}),
    onDataVersion: () => () => undefined,
  };
  mount(host, { instance: 'bmi-gue-s4' }, shell, timing);
  await flush();
  assert.match(host.querySelector('#battlemap-status')?.textContent ?? '', /Impossible de lire la battlemap/);
});

test('a token dragged off the image is put back, and nothing is sent', async () => {
  const { document, fake, pointer, at, token } = await battlemap({ view: 'dm' });
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), { x: 5000, y: from.y });
  pointer('pointerup', token('t-elowen'), { x: 5000, y: from.y });
  await flush();
  assert.equal(fake.sent.length, 0);
  assert.equal(left(token('t-elowen')), px(from.x));
  assert.match(document.querySelector('#battlemap-status')?.textContent ?? '', /hors de la carte/);
});

test('a character dropped off the image opens no dialog', async () => {
  const { document, dom, fake } = await battlemap({ view: 'dm' });
  const event = new dom.window.Event('drop', { bubbles: true, cancelable: true });
  Object.assign(event, { clientX: 9000, clientY: 9000, dataTransfer: { getData: () => 'bandit' } });
  (document.querySelector('#battlemap-view') as HTMLElement).dispatchEvent(event);
  assert.equal(document.querySelector('#battlemap-drop'), null);
  assert.equal(fake.sent.length, 0);
});

test('the commands target the mounted instance even when the answer names none', async () => {
  const { fake, pointer, at, token } = await battlemap({ view: 'dm' }, { omitId: true });
  const from = at(0, 6);
  pointer('pointerdown', token('t-elowen'), from);
  pointer('pointermove', token('t-elowen'), { x: from.x + 10, y: from.y });
  pointer('pointermove', token('t-elowen'), at(1, 6));
  pointer('pointerup', token('t-elowen'), at(1, 6));
  await flush();
  assert.equal(fake.sent[0]?.target.id, 'bmi-gue-s4');
});

test('props that change under a drag do not carry the drag to the next instance', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell();
  const mounted = mount(host, { instance: 'bmi-gue-s4', view: 'dm' }, fake.shell, timing);
  await flush();
  const el = document.querySelector('#token-t-elowen') as HTMLElement;
  const from = center(map.grid, { q: 0, r: 6 });
  const fire = (type: string, target: Element, x: number) => target.dispatchEvent(new dom.window.MouseEvent(type, { bubbles: true, button: 0, clientX: x, clientY: from.y }));
  fire('pointerdown', el, from.x);
  fire('pointermove', el, from.x + 20);
  fire('pointermove', el, from.x + 90);
  mounted.update({ instance: 'bmi-gue-s4', view: 'player' });
  await flush();
  fire('pointerup', host, from.x + 90);
  await flush();
  assert.equal(fake.sent.length, 0);
});

test('a battlemap the media library cannot give leaves nothing to send', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell();
  const shell: Shell = {
    ...fake.shell,
    capability: async (id, sha, variables) => {
      if (id === 'media.getBattlemap') throw new Error('down');
      return fake.shell.capability(id, sha, variables);
    },
  };
  mount(host, { instance: 'bmi-gue-s4' }, shell, timing);
  await flush();
  assert.match(host.querySelector('#battlemap-status')?.textContent ?? '', /Impossible de lire la battlemap/);
  assert.equal(host.querySelectorAll('.kit-token__item').length, 0);
  host.querySelector<HTMLElement>('#bm-send')?.click();
  await flush();
  assert.equal(fake.sent.length, 0);
});

// The frame {1.113, 196, 16} in the 984 × 656 view: the fit's scale (1.025) times the zoom, the map point 196 px right of and 16 px below the map's centre in the view's centre.
const STAGE_OF_PLAYERS = 'width: 960px; height: 640px; transform: translate(-279.198px, -55.317px) scale(1.140825);';
const showing = (view: unknown, id = 'bmi-gue-s4') => ({ base: { kind: 'battlemap', id }, window: null, view });

// AC1: the pilot tab's zoom, wheel and pan are sent to the players, rounded, and the pilot's stage carries the same values
test('the pilot tab sends its zoom, wheel and pan once they rest, rounded as the diffusion takes them, and shows them on its own stage', async () => {
  const { document, fake, pointer, dom } = await battlemap({ view: 'player', pilot: true, fill: true }, undefined, undefined, PLAYER_VIEW);
  const stage = document.querySelector<HTMLElement>('#battlemap-stage') as HTMLElement;
  const view = document.querySelector<HTMLElement>('#battlemap-view') as HTMLElement;
  // No pause between the three gestures: they rest together.
  document.querySelector<HTMLElement>('[data-act="in"]')?.click();
  view.dispatchEvent(new dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -120, clientX: 105, clientY: 51 }));
  pointer('pointerdown', view, { x: 10, y: 10 });
  pointer('pointermove', view, { x: 40, y: 25 });
  pointer('pointerup', view, { x: 40, y: 25 });
  await flush();
  const frames = fake.sent.filter((c) => c.dataCapability === 'live.showOnPlayerView@1');
  assert.equal(frames.length, 1, 'the last gesture wins: one frame for the three');
  const { kind, instance, zoom, panX, panY } = frames[0]?.payload as Record<string, number | string>;
  assert.equal(kind, 'battlemap');
  assert.equal(instance, 'bmi-gue-s4');
  assert.ok(Number.isInteger(Number(panX)) && Number.isInteger(Number(panY)), 'whole pixels');
  assert.equal(Number(zoom), Number(Number(zoom).toFixed(3)), 'three decimals at most');
  assert.ok(Number(zoom) > 1.3, 'the zoom of the button and of the wheel together');
  // The stage shows the frame sent, laid out in this view's own pixels.
  const at = placeStage({ zoom: Number(zoom), panX: Number(panX), panY: Number(panY) }, PLAYER_VIEW, map);
  assert.equal(stage.style.transform, `translate(${Number(at.x.toFixed(3))}px, ${Number(at.y.toFixed(3))}px) scale(${Number(at.scale.toFixed(6))})`);
  assert.equal(document.querySelector('#battlemap-status')?.hasAttribute('hidden'), true, 'a frame sent on its own says nothing');
});

// AC1: a frame the server refuses is told, not hidden
test('a frame the server refuses is told in the pilot tab', async () => {
  const { document, fake, click } = await battlemap({ view: 'player', pilot: true, fill: true }, { reject: 'base-coherent' });
  await click('[data-act="out"]');
  assert.equal(fake.sent.length, 1);
  assert.match(document.querySelector('#battlemap-status')?.textContent ?? '', /L’envoi aux joueurs est refusé/);
});

test('the DM view and the projected view send no frame of their own', async () => {
  for (const props of [{ view: 'dm' }, { view: 'player' }]) {
    const { fake, document, dom } = await battlemap(props);
    document.querySelector<HTMLElement>('[data-act="in"]')?.click();
    document.querySelector('#battlemap-view')?.dispatchEvent(new dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -120 }));
    await flush();
    assert.equal(fake.sent.length, 0, JSON.stringify(props));
  }
});

// AC2: the projected view takes the diffusion's frame, and follows its later change
test('the projected view applies the diffusion’s view to its stage and follows a later change', async () => {
  const { document, fake } = await battlemap({ view: 'player' }, { diffusion: showing({ zoom: 1.113, panX: 196, panY: 16 }) }, undefined, PLAYER_VIEW);
  const stage = document.querySelector<HTMLElement>('#battlemap-stage') as HTMLElement;
  assert.deepEqual(fake.reads.find((r) => r.id === 'live.getDiffusion')?.variables, { view: 'player' });
  assert.equal(stage.getAttribute('style'), STAGE_OF_PLAYERS);
  assert.ok(document.querySelector('[data-reads~="live.getDiffusion"]'), 'a part names what it reads');
  fake.diffused.value = showing({ zoom: 2, panX: -40, panY: 8 });
  fake.moved();
  await flush();
  assert.equal(stage.getAttribute('style'), 'width: 960px; height: 640px; transform: translate(-410px, -344.4px) scale(2.05);');
  assert.equal(fake.sent.length, 0);
});

// AC3: with no view, or another instance, the map stays fitted
test('the projected view stays fitted when the diffusion has no view or shows another instance', async () => {
  for (const diffusion of [showing(null), showing({ zoom: 1.113, panX: 196, panY: 16 }, 'bmi-other'), { base: { kind: 'cover', id: 'ill-cover' }, window: null, view: { zoom: 1.113, panX: 196, panY: 16 } }]) {
    const { document } = await battlemap({ view: 'player' }, { diffusion }, undefined, PLAYER_VIEW);
    const style = document.querySelector('#battlemap-stage')?.getAttribute('style') ?? '';
    assert.doesNotMatch(style, /scale\(1\.1/, JSON.stringify(diffusion));
    assert.match(style, /translate\(0px, 0px\) scale\(1\.025\)/, 'the map is as it was fitted: centred at the fit');
  }
});

// AC3: a view that goes away puts the fit back
test('the projected view is fitted again once the diffusion drops its view', async () => {
  const { document, fake } = await battlemap({ view: 'player' }, { diffusion: showing({ zoom: 1.113, panX: 196, panY: 16 }) }, undefined, PLAYER_VIEW);
  const stage = document.querySelector<HTMLElement>('#battlemap-stage') as HTMLElement;
  assert.equal(stage.getAttribute('style'), STAGE_OF_PLAYERS);
  fake.diffused.value = showing(null);
  fake.moved();
  await flush();
  assert.match(stage.getAttribute('style') ?? '', /translate\(0px, 0px\) scale\(1\.025\)/);
});

// The token's place in its view, as a fraction of it: where the stage puts the token's map point (the left and top it is drawn at).
const relativePlace = (document: Document, id: string, view: { width: number; height: number }) => {
  const stage = document.querySelector<HTMLElement>('#battlemap-stage') as HTMLElement;
  const [, x, y, scale] = /translate\((-?[\d.]+)px, (-?[\d.]+)px\) scale\(([\d.]+)\)/.exec(stage.style.transform) ?? [];
  const disc = document.querySelector<HTMLElement>(`#token-${id}`) as HTMLElement;
  return { x: (parseFloat(disc.style.left) * Number(scale) + Number(x)) / view.width, y: (parseFloat(disc.style.top) * Number(scale) + Number(y)) / view.height };
};
const close = (a: { x: number; y: number }, b: { x: number; y: number }) => Math.abs(a.x - b.x) < 0.02 && Math.abs(a.y - b.y) < 0.02;

// AC2: a drop on the DM view is at the same place relative to the view on the player view, whatever the two sizes
test('a token is at the same relative place in a view of any size, fitted', async () => {
  const dm = await battlemap({ view: 'dm' }, undefined, undefined, DM_VIEW);
  const player = await battlemap({ view: 'player' }, undefined, undefined, PLAYER_VIEW);
  const narrow = await battlemap({ view: 'player' }, undefined, undefined, { width: 300, height: 200 });
  const at = relativePlace(dm.document, 't-elowen', DM_VIEW);
  assert.ok(at.x > 0 && at.x < 1 && at.y > 0 && at.y < 1, 'in the view');
  assert.ok(close(at, relativePlace(player.document, 't-elowen', PLAYER_VIEW)), 'the projected view');
  assert.ok(close(at, relativePlace(narrow.document, 't-elowen', { width: 300, height: 200 })), 'a small one');
});

// AC1: the frame the pilot tab sends shows the same part of the map on a view of another size
test('the same frame puts the map at the same relative place in the pilot tab, the projected view and the DM view, whatever their sizes', async () => {
  const pilot = await battlemap({ view: 'player', pilot: true, fill: true }, undefined, undefined, PLAYER_VIEW);
  pilot.document.querySelector<HTMLElement>('[data-act="in"]')?.click();
  pilot.document.querySelector('#battlemap-view')?.dispatchEvent(new pilot.dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -120, clientX: 300, clientY: 200 }));
  pilot.pointer('pointerdown', pilot.document.querySelector('#battlemap-view') as Element, { x: 10, y: 10 });
  pilot.pointer('pointermove', pilot.document.querySelector('#battlemap-view') as Element, { x: 70, y: 30 });
  pilot.pointer('pointerup', pilot.document.querySelector('#battlemap-view') as Element, { x: 70, y: 30 });
  await flush();
  const sent = pilot.fake.sent.filter((c) => c.dataCapability === 'live.showOnPlayerView@1');
  const { zoom, panX, panY } = sent[0]?.payload as { zoom: number; panX: number; panY: number };
  assert.ok(zoom > 1.3 && panX < 0 && Number.isInteger(panX), 'the pan is whole map pixels, and the map moved with the pointer');
  const diffusion = showing({ zoom, panX, panY });
  const projected = await battlemap({ view: 'player' }, { diffusion }, undefined, PLAYER_VIEW);
  const small = await battlemap({ view: 'player' }, { diffusion }, undefined, DM_VIEW);
  const mine = relativePlace(pilot.document, 't-elowen', PLAYER_VIEW);
  assert.equal(projected.document.querySelector('#battlemap-stage')?.getAttribute('style'), pilot.document.querySelector('#battlemap-stage')?.getAttribute('style'), 'the same view size, the same stage');
  assert.ok(close(mine, relativePlace(projected.document, 't-elowen', PLAYER_VIEW)));
  assert.ok(close(mine, relativePlace(small.document, 't-elowen', DM_VIEW)), 'a view of another size shows the same part of the map');
});

// AC1: a pan is in map pixels, whatever the view's scale
test('a pan of the pointer moves the map by that distance in the view, whatever its scale', async () => {
  const { document, pointer, fake } = await battlemap({ view: 'player', pilot: true, fill: true }, undefined, undefined, DM_VIEW);
  const view = document.querySelector('#battlemap-view') as Element;
  const before = relativePlace(document, 't-elowen', DM_VIEW);
  pointer('pointerdown', view, { x: 100, y: 100 });
  pointer('pointermove', view, { x: 170, y: 140 });
  pointer('pointerup', view, { x: 170, y: 140 });
  await flush();
  const after = relativePlace(document, 't-elowen', DM_VIEW);
  // Within the whole map pixel the frame is rounded to.
  assert.ok(Math.abs(after.x - before.x - 70 / DM_VIEW.width) < 0.002 && Math.abs(after.y - before.y - 40 / DM_VIEW.height) < 0.003);
  // 70 view pixels at the fit's scale (705 / 960): whole map pixels, the other way.
  const { panX, panY } = fake.sent.find((c) => c.dataCapability === 'live.showOnPlayerView@1')?.payload as { panX: number; panY: number };
  assert.deepEqual([panX, panY], [Math.round(-70 / 0.734375), Math.round(-40 / 0.734375)]);
});

// AC1: a view that changes size keeps its frame, laid out again
test('a view that is resized lays the same frame out again, and sends nothing', async () => {
  const { document, fake, resize } = await battlemap({ view: 'player' }, { diffusion: showing({ zoom: 2, panX: 100, panY: -50 }) }, undefined, DM_VIEW);
  const before = relativePlace(document, 't-elowen', DM_VIEW);
  const style = document.querySelector('#battlemap-stage')?.getAttribute('style');
  resize(PLAYER_VIEW);
  assert.notEqual(document.querySelector('#battlemap-stage')?.getAttribute('style'), style);
  assert.ok(close(before, relativePlace(document, 't-elowen', PLAYER_VIEW)));
  assert.equal(fake.sent.length, 0);
});

// AC1: the view keeps the map's ratio
test('the view keeps the ratio of the map, so no view of another shape shows another part', async () => {
  const { document } = await battlemap({ view: 'dm' }, undefined, undefined, DM_VIEW);
  const view = document.querySelector<HTMLElement>('#battlemap-view') as HTMLElement;
  assert.equal(view.style.aspectRatio.replace(/\s/g, ''), `${map.width}/${map.height}`);
  assert.equal(view.style.getPropertyValue('--bm-ratio'), String(map.width / map.height));
});

/** Where the CSS leaves an element: jsdom lays nothing out, but it does cascade the stylesheet the Micro-UI injects. */
const positioned = (dom: JSDOM, selector: string) => dom.window.getComputedStyle(dom.window.document.querySelector(selector) as Element).position;

/** Where the tokens and the stage stand: what moves when the map is refitted. */
const snapshot = (document: Document, token: (id: string) => HTMLElement) =>
  ['t-elowen', 't-kael', 't-bandit-2'].map((id) => `${left(token(id))}/${token(id).style.top}`).concat(document.querySelector<HTMLElement>('#battlemap-stage')?.style.transform ?? '');

// AC1: opening a token's sheet on the DM view leaves Elowen where she was relative to the view
test('the DM view lays the token’s sheet over the map, so opening it resizes nothing and no token moves', async () => {
  const { dom, document, token, pointer, at } = await battlemap({ view: 'dm' }, undefined, 'http://localhost/', { width: 705, height: 520 });
  assert.equal(positioned(dom, '#token-inspector'), 'absolute', 'the sheet is out of the flow: the map keeps the room it has');
  assert.equal(positioned(dom, '#battlemap-body'), 'relative', 'the sheet is laid over the body, whose size it does not change');
  const before = snapshot(document, token);
  pointer('pointerdown', token('t-bandit-2'), at(10, 6));
  pointer('pointerup', token('t-bandit-2'), at(10, 6));
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), false, 'the sheet is open');
  assert.equal(document.querySelector('#inspector-title')?.textContent, 'Bandit 2');
  assert.equal(positioned(dom, '#token-inspector'), 'absolute');
  assert.deepEqual(snapshot(document, token), before);
});

test('the sheet is over the map whatever the screen’s mode', async () => {
  for (const props of [{ view: 'dm' }, { view: 'dm', fill: true }, { view: 'player', pilot: true }, { view: 'prep' }]) {
    const { dom } = await battlemap(props);
    assert.equal(positioned(dom, '#token-inspector'), 'absolute', JSON.stringify(props));
  }
});

// AC3: closing the sheet moves no token relative to the view
test('closing the token’s sheet moves no token', async () => {
  const { dom, document, token, pointer, at, click } = await battlemap({ view: 'dm' }, undefined, 'http://localhost/', { width: 705, height: 520 });
  const before = snapshot(document, token);
  pointer('pointerdown', token('t-bandit-2'), at(10, 6));
  pointer('pointerup', token('t-bandit-2'), at(10, 6));
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), false, 'the sheet is open');
  await click('[data-act="close"]');
  assert.equal(document.querySelector('#token-inspector')?.hasAttribute('hidden'), true, 'the sheet is closed');
  assert.deepEqual(snapshot(document, token), before);
  assert.equal(positioned(dom, '#token-inspector'), 'absolute');
});
