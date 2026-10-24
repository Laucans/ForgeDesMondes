import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { center } from '../src/hexgrid.ts';
import { mount, type Timing } from '../src/index.ts';
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
  return { ...fx, grid: { ...fx.grid, opacity: fx.grid.opacity / 100 } } as { id: string; grid: { size: number; ox: number; oy: number; orient: 'pointy'; color: string; opacity: number } };
})();

/** A shell over a small table: it answers the three reads and applies the commands as the table would. */
function fakeShell(options: { tokens?: (all: Array<Record<string, unknown>>) => Array<Record<string, unknown>>; reject?: string; leaky?: boolean; omitId?: boolean } = {}) {
  const instance = structuredClone(fixtureOf('table.getBattlemapInstance')) as Raw;
  if (options.tokens) instance.state.tokens = options.tokens(instance.state.tokens);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables: Record<string, unknown> | undefined }> = [];
  const find = (id: unknown) => instance.state.tokens.find((t) => t.id === id) as Record<string, unknown>;
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, variables });
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      if (id === 'media.getBattlemap') return { asOf: 1, data: map };
      if (id === 'campaign.listCharacters') return { asOf: 1, data: fixtureOf('campaign.listCharacters') };
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
    onDataVersion: () => () => undefined,
  };
  return { shell, sent, reads, instance };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5, watchMs: 1000 };
const flush = async (): Promise<void> => {
  for (let i = 0; i < 12; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

async function battlemap(props: Record<string, unknown>, options?: Parameters<typeof fakeShell>[0]) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const { document } = dom.window;
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
  return { dom, document, host, fake, pointer, at, token, click };
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
    caller: { by: 'dm', from: 'Battlemap', roles: [] },
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
  const { document, fake, click } = await battlemap({ view: 'dm' });
  await click('#bm-send');
  assert.deepEqual(fake.sent[0], {
    dataCapability: 'live.showOnPlayerView@1',
    target: { aggregate: 'Diffusion', id: 's4' },
    payload: { kind: 'battlemap', instance: 'bmi-gue-s4', zoom: 1, panX: 0, panY: 0 },
    caller: { by: 'dm', from: 'Battlemap', roles: [] },
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
