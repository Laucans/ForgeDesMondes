import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import { STYLE } from '../src/style.ts';
import type { Shell } from '../src/types.ts';
import { assertDeclared } from './manifests.ts';

const fixtureOf = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));

type Raw = Record<string, unknown>;
interface Order {
  id: string;
  init: number;
  dex: number;
}
interface Combat {
  round: number;
  current: string;
  order: Order[];
}
interface Table {
  id: string;
  state: { tokens: Raw[]; combat: Combat | null };
  [key: string]: unknown;
}
interface Sent {
  dataCapability: string;
  target: { aggregate: string; id?: string };
  payload: Raw;
}

const map = (() => {
  const fx = fixtureOf('media.getBattlemap') as { grid: { opacity: number } };
  return { ...fx, grid: { ...fx.grid, opacity: fx.grid.opacity / 100 } };
})();

const initial = (): Table => structuredClone(fixtureOf('table.getBattlemapInstance')) as Table;

/** A combat already under way on the fixture's tokens: Elowen, the boss (hidden) second, then two bandits. */
const underWay = (): Combat => ({
  round: 1,
  current: 't-elowen',
  order: [
    { id: 't-elowen', init: 18, dex: 4 },
    { id: 't-vesna-1', init: 15, dex: 3 },
    { id: 't-bandit-1', init: 12, dex: 1 },
    { id: 't-bandit-2', init: 9, dex: 1 },
    { id: 't-bandit-3', init: 4, dex: 1 },
  ],
});

/** A shell over a small table that applies the combat commands the way the DataGuard's invariants say, and answers each view as it should. */
function fakeShell(options: { combat?: Combat | null; tokens?: (all: Raw[]) => Raw[]; wire?: boolean } = {}) {
  const table = initial();
  if (options.combat !== undefined) table.state.combat = options.combat;
  if (options.tokens) table.state.tokens = options.tokens(table.state.tokens);
  const first = initial();
  const sent: Sent[] = [];
  const find = (id: unknown): Raw => table.state.tokens.find((t) => t.id === id) as Raw;
  /** next-turn, as the table keeps it: the next living participant by rank, the round counting up on a lap. */
  const advance = (combat: Combat): void => {
    const from = combat.order.findIndex((o) => o.id === combat.current);
    for (let step = 1; step <= combat.order.length; step += 1) {
      const at = (from + step) % combat.order.length;
      const next = combat.order[at] as Order;
      if (find(next.id).hp !== 0) {
        if (at <= from) combat.round += 1;
        combat.current = next.id;
        return;
      }
    }
  };
  const shell: Shell = {
    async capability(id, _sha, variables) {
      if (id === 'media.getBattlemap') return { asOf: 1, data: map };
      if (id === 'campaign.listCharacters') return { asOf: 1, data: fixtureOf('campaign.listCharacters') };
      const own = structuredClone(table);
      for (const t of own.state.tokens) if (t.kind === 'pnj') t.dead = t.hp === 0;
      if (variables?.view === 'player') {
        own.state.tokens = own.state.tokens.filter((t) => t.visible).map(({ hp, hpMax, ...t }) => ({ ...t, healthBand: t.kind === 'pnj' ? (hp === hpMax ? 'indemne' : 'blessé') : null }));
        const seen = new Set(own.state.tokens.map((t) => t.id));
        if (own.state.combat) own.state.combat.order = own.state.combat.order.filter((o) => seen.has(o.id));
      }
      // The Capability's own shape: each participant of the order carries `initiative`, not `init`.
      if (options.wire && own.state.combat) own.state.combat.order = own.state.combat.order.map(({ id, init }) => ({ id, initiative: init })) as unknown as Order[];
      return { asOf: 1, data: own };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      assertDeclared(command);
      const { payload } = command;
      const combat = table.state.combat;
      switch (command.dataCapability.replace(/@\d+$/, '')) {
        case 'table.changeHitPoints': {
          const t = find(payload.token);
          if (t.kind !== 'pnj') break;
          t.hp = Math.max(0, Math.min(Number(t.hpMax), Number(t.hp) + Number(payload.delta)));
          break;
        }
        case 'table.startCombat': {
          const rolls = payload.rolls as Record<string, number>;
          const entered = payload.initiative as Record<string, number>;
          const order = table.state.tokens
            .filter((t) => t.hp !== 0)
            .map((t) => ({ id: String(t.id), init: t.kind === 'pnj' ? (rolls[String(t.id)] ?? 0) + 1 : (entered[String(t.char)] ?? 0), dex: 1 }))
            .sort((a, b) => b.init - a.init);
          table.state.combat = { round: 1, current: order[0]?.id ?? '', order };
          break;
        }
        case 'table.nextTurn':
          if (combat) advance(combat);
          break;
        case 'table.reorderInitiative': {
          if (!combat) break;
          combat.order = (payload.order as string[]).map((id) => combat.order.find((o) => o.id === id) as Order);
          break;
        }
        case 'table.addParticipant':
          combat?.order.push({ id: String(payload.token), init: Number(payload.initiative ?? 10), dex: 0 });
          break;
        case 'table.removeParticipant':
          if (!combat) break;
          // The table refuses the participant that holds the turn: the caller passes it on first.
          if (combat.current === payload.participant) return { commandId: `c${sent.length}`, result: { status: 'rejected', violations: ['next-turn'] } };
          combat.order = combat.order.filter((o) => o.id !== payload.participant);
          break;
        case 'table.removeToken':
          // The turn is nobody's once its token is gone: the table asks the caller to pass it on first.
          if (combat?.current === payload.token) return { commandId: `c${sent.length}`, result: { status: 'rejected', violations: ['next-turn'] } };
          table.state.tokens = table.state.tokens.filter((t) => t.id !== payload.token);
          if (combat) combat.order = combat.order.filter((o) => o.id !== payload.token);
          break;
        case 'table.endCombat':
          table.state.combat = null;
          break;
        case 'table.resetBattlemap':
          table.state = first.state;
          break;
        case 'table.setTokenVisibility':
          find(payload.token).visible = payload.visible;
          break;
        default:
      }
      return { commandId: `c${sent.length}`, result: { status: 'applied', violations: [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: 'applied', violations: [] } };
    },
    onDataVersion: () => () => undefined,
  };
  return { shell, sent, table };
}

// The d20 an NPC rolls: 10 each, so the fixture's NPC reach 10 + their DEX modifier.
const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5, watchMs: 1000, roll: () => 10 };
const flush = async (): Promise<void> => {
  for (let i = 0; i < 12; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

async function battlemap(props: Raw, options?: Parameters<typeof fakeShell>[0], size?: { width: number; height: number }) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="Battlemap"></div></body></html>');
  const { document } = dom.window;
  // jsdom lays nothing out: the map's view is the size the test gives it, when it gives one.
  if (size) {
    Object.defineProperty(dom.window.HTMLElement.prototype, 'clientWidth', { configurable: true, get(this: HTMLElement) { return this.id === 'battlemap-view' ? size.width : 0; } });
    Object.defineProperty(dom.window.HTMLElement.prototype, 'clientHeight', { configurable: true, get(this: HTMLElement) { return this.id === 'battlemap-view' ? size.height : 0; } });
  }
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, { instance: 'bmi-gue-s4', ...props }, fake.shell, timing);
  await flush();
  const $ = <T extends HTMLElement = HTMLElement>(selector: string): T => document.querySelector(selector) as T;
  const click = async (selector: string): Promise<void> => {
    $(selector).click();
    await flush();
  };
  const carousel = (): string[] => Array.from(document.querySelectorAll('#carousel-track .kit-token__item')).map((el) => (el as HTMLElement).dataset.token ?? '');
  const open = async (id: string): Promise<void> => click(`#carousel-${id}`);
  /** A press on a token of the map, with no drag: its sheet opens. */
  const pick = async (id: string): Promise<void> => {
    const el = $(`#token-${id}`);
    for (const type of ['pointerdown', 'pointerup']) el.dispatchEvent(new dom.window.MouseEvent(type, { bubbles: true, cancelable: true, button: 0 }));
    await flush();
  };
  const amount = async (value: string): Promise<void> => {
    $<HTMLInputElement>('#inspector-hp-amount').value = value;
  };
  const drag = (type: string, target: Element, data: Record<string, string>): Event => {
    const event = new dom.window.Event(type, { bubbles: true, cancelable: true });
    Object.assign(event, { dataTransfer: { types: Object.keys(data), getData: (t: string) => data[t] ?? '', setData: (t: string, v: string) => (data[t] = v), effectAllowed: '', dropEffect: '' } });
    target.dispatchEvent(event);
    return event;
  };
  return { dom, document, fake, $, click, carousel, open, pick, amount, drag };
}

test('with no combat the DM can start one and no carousel is drawn', async () => {
  const { $, document } = await battlemap({ view: 'dm' });
  assert.equal($('#initiative-carousel').hidden, true);
  assert.equal($('#bm-start-combat').textContent, 'Lancer le combat');
  assert.equal(document.querySelector('#bm-end-combat'), null);
  assert.equal($('#bm-start-combat').dataset.writes, 'table.startCombat');
  assert.equal($('#bm-reset').dataset.writes, 'table.resetBattlemap');
});

test('« Lancer le combat » rolls the NPC, asks the PC initiative, and starts the combat with both', async () => {
  const { $, document, click, fake, carousel } = await battlemap({ view: 'dm' });
  await click('#bm-start-combat');
  assert.equal(fake.sent.length, 0, 'nothing is sent before the DM validates');
  const rows = Array.from(document.querySelectorAll('#combat-setup .kit-dialog__row')).map((r) => r.textContent?.replace(/\s+/g, ' ').trim());
  assert.match(rows.find((r) => r?.startsWith('Bandit 1')) ?? '', /d20 \(10\) \+ 1 = 11/);
  assert.match(rows.find((r) => r?.startsWith('Chef bandit 1')) ?? '', /\(caché\)/, 'a hidden token is listed as such');
  assert.equal($<HTMLInputElement>('[data-init="t-elowen"]').value, '18', 'the PC initiative the DM gave before is proposed');
  $<HTMLInputElement>('[data-init="t-kael"]').value = '15';
  await click('#combat-start-submit');
  assert.equal(document.querySelector('#combat-start-dialog'), null);
  assert.equal(fake.sent.length, 1);
  const [command] = fake.sent;
  assert.equal(command?.dataCapability, 'table.startCombat@1');
  assert.deepEqual(command?.payload, {
    initiative: { elowen: 18, kael: 15, brannoc: 9, sirielle: 14 },
    rolls: { 't-bandit-1': 10, 't-bandit-2': 10, 't-bandit-3': 10, 't-vesna-1': 10 },
  });
  assert.equal($('#initiative-carousel').hidden, false);
  assert.equal($('#combat-round b').textContent, '1');
  assert.equal(carousel()[0], 't-elowen');
  assert.equal($('#bm-end-combat').textContent, 'Terminer le combat', 'the button now ends the combat');
});

test('cancelling the start dialog sends nothing', async () => {
  const { document, click, fake } = await battlemap({ view: 'dm' });
  await click('#bm-start-combat');
  await click('#combat-start-cancel');
  assert.equal(document.querySelector('#combat-start-dialog'), null);
  assert.equal(fake.sent.length, 0);
});

test('an NPC’s sheet changes its HP by -4: one command, for that token only, and the sheet shows the new HP', async () => {
  const { $, pick, click, amount, fake } = await battlemap({ view: 'dm' });
  await pick('t-bandit-2');
  assert.match($('#inspector-hp').textContent ?? '', /11 \/ 11 PV/);
  await amount('4');
  await click('#inspector-damage');
  assert.equal(fake.sent.length, 1);
  assert.equal(fake.sent[0]?.dataCapability, 'table.changeHitPoints@1');
  assert.deepEqual(fake.sent[0]?.payload, { token: 't-bandit-2', delta: -4 });
  assert.match($('#inspector-hp').textContent ?? '', /7 \/ 11 PV/);
  assert.match($('#inspector-hp').textContent ?? '', /Vu par les joueurs : Blessé/);
  for (const other of ['t-bandit-1', 't-bandit-3']) assert.match($(`#token-${other}`).textContent ?? '', /11\/11 PV/, 'the other instances are untouched');
  assert.equal($<HTMLInputElement>('#inspector-hp-amount').value, '4', 'the amount the DM typed stays for the next change');
});

test('healing is a change of the opposite sign and stops at the maximum', async () => {
  const { pick, click, amount, fake, $ } = await battlemap({ view: 'dm' });
  await pick('t-bandit-2');
  await amount('4');
  await click('#inspector-damage');
  await amount('9');
  await click('#inspector-heal');
  assert.deepEqual(fake.sent.map((c) => c.payload.delta), [-4, 9]);
  assert.match($('#inspector-hp').textContent ?? '', /11 \/ 11 PV/);
});

test('an amount of zero, or none, sends nothing', async () => {
  const { pick, click, amount, fake } = await battlemap({ view: 'dm' });
  await pick('t-bandit-2');
  await amount('0');
  await click('#inspector-damage');
  await amount('');
  await click('#inspector-heal');
  assert.equal(fake.sent.length, 0);
});

test('pj-hp-not-tracked: a PC’s sheet has no HP to change', async () => {
  const { $, pick, document } = await battlemap({ view: 'dm' });
  await pick('t-elowen');
  assert.equal(document.querySelector('#inspector-hp'), null);
  assert.equal(document.querySelector('#inspector-damage'), null);
  assert.match($('#inspector-pj').textContent ?? '', /ne sont pas suivis/);
});

test('an NPC at 0 HP is greyed on the map and in the carousel, and « Suivant » skips it', async () => {
  const { $, pick, click, amount, fake, document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await pick('t-bandit-2');
  await amount('11');
  await click('#inspector-damage');
  assert.ok($('#token-t-bandit-2').classList.contains('kit-token__item--dead'), 'greyed on the map');
  assert.ok($('#carousel-t-bandit-2').classList.contains('kit-token__item--dead'), 'and in the carousel');
  assert.ok(document.querySelector('#inspector-dead'));
  const turns: string[] = [];
  for (let i = 0; i < 5; i += 1) {
    await click('#combat-next');
    turns.push(document.querySelector('.kit-token__item--current')?.getAttribute('data-token') ?? '');
  }
  assert.deepEqual(turns, ['t-vesna-1', 't-bandit-1', 't-bandit-3', 't-elowen', 't-vesna-1'], 'the dead bandit never has the turn');
  assert.ok(fake.sent.filter((c) => c.dataCapability === 'table.nextTurn@1').length === 5);
});

test('« Suivant » submits table.nextTurn and the carousel highlights the next one; the round counts up after a lap', async () => {
  const { $, click, fake } = await battlemap({ view: 'dm' }, { combat: { ...underWay(), current: 't-bandit-3' } });
  assert.ok($('#carousel-t-bandit-3').classList.contains('kit-token__item--current'));
  await click('#combat-next');
  assert.deepEqual(fake.sent[0]?.payload, {});
  assert.equal(fake.sent[0]?.dataCapability, 'table.nextTurn@1');
  assert.ok($('#carousel-t-elowen').classList.contains('kit-token__item--current'));
  assert.equal($('#combat-round b').textContent, '2');
});

test('a participant dragged onto another takes its rank: table.reorderInitiative', async () => {
  const { $, drag, fake, carousel } = await battlemap({ view: 'dm' }, { combat: underWay() });
  const data: Record<string, string> = {};
  drag('dragstart', $('#carousel-t-bandit-3'), data);
  assert.equal(data['text/fdm-participant'], 't-bandit-3');
  assert.equal(drag('dragover', $('#carousel-t-vesna-1'), data).defaultPrevented, true, 'a participant may be dropped on another');
  const drop = drag('drop', $('#carousel-t-vesna-1'), data);
  assert.equal(drop.defaultPrevented, true);
  await flush();
  assert.equal(fake.sent[0]?.dataCapability, 'table.reorderInitiative@1');
  assert.deepEqual(fake.sent[0]?.payload, { order: ['t-elowen', 't-bandit-3', 't-vesna-1', 't-bandit-1', 't-bandit-2'] });
  assert.deepEqual(carousel(), ['t-elowen', 't-bandit-3', 't-vesna-1', 't-bandit-1', 't-bandit-2']);
});

test('a participant dropped where it already stands sends nothing', async () => {
  const { $, drag, fake } = await battlemap({ view: 'dm' }, { combat: underWay() });
  const data: Record<string, string> = {};
  drag('dragstart', $('#carousel-t-bandit-1'), data);
  drag('drop', $('#carousel-t-bandit-1'), data);
  await flush();
  assert.equal(fake.sent.length, 0);
});

test('the sheet’s « Avancer » and « Reculer » move the chosen participant by one rank', async () => {
  const { $, click, open, fake, carousel } = await battlemap({ view: 'dm' }, { combat: underWay() });
  assert.equal($<HTMLButtonElement>('#combat-earlier').disabled, true, 'nobody chosen');
  await open('t-bandit-1');
  await click('#combat-earlier');
  assert.deepEqual(fake.sent[0]?.payload, { order: ['t-elowen', 't-bandit-1', 't-vesna-1', 't-bandit-2', 't-bandit-3'] });
  await click('#combat-later');
  assert.deepEqual(fake.sent[1]?.payload, { order: ['t-elowen', 't-vesna-1', 't-bandit-1', 't-bandit-2', 't-bandit-3'] });
  assert.deepEqual(carousel(), ['t-elowen', 't-vesna-1', 't-bandit-1', 't-bandit-2', 't-bandit-3']);
  await open('t-elowen');
  await click('#combat-earlier');
  assert.equal(fake.sent.length, 2, 'the first cannot go before the first');
});

// AC2: a combat saved with Bandit 2's sheet open is reopened with the tokens at the same places relative to the view
test('a combat reopened with a sheet open lays the map out as it was, the sheet over it', async () => {
  const where = (document: Document) => ['t-elowen', 't-bandit-2'].map((id) => { const el = document.querySelector<HTMLElement>(`#token-${id}`); return `${el?.style.left}/${el?.style.top}`; });
  const closed = await battlemap({ view: 'dm' }, { combat: underWay() }, { width: 705, height: 520 });
  const reopened = await battlemap({ view: 'dm' }, { combat: underWay() }, { width: 705, height: 520 });
  await reopened.open('t-bandit-2');
  assert.equal(reopened.$('#inspector-title').textContent, 'Bandit 2');
  assert.deepEqual(where(reopened.document), where(closed.document), 'the sheet changes no place');
  assert.equal(reopened.$('#battlemap-stage').style.transform, closed.$('#battlemap-stage').style.transform);
  assert.match(STYLE, /\.bm #token-inspector \{ position: absolute;/);
});

test('a click on a participant opens the sheet of that very instance', async () => {
  const { $, open, document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await open('t-bandit-2');
  assert.equal($('#inspector-title').textContent, 'Bandit 2');
  assert.equal($('#token-inspector').getAttribute('aria-labelledby'), 'inspector-title');
  assert.match($('#token-inspector').textContent ?? '', /initiative 9/);
  assert.equal(document.querySelectorAll('#inspector-instances .chip').length, 3, 'the other instances are one click away');
});

test('removing a participant, adding one back and ending the combat each submit their DataCapability', async () => {
  const { $, click, open, fake, carousel, document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await open('t-bandit-2');
  await click('#combat-drop');
  assert.deepEqual([fake.sent[0]?.dataCapability, fake.sent[0]?.payload], ['table.removeParticipant@1', { participant: 't-bandit-2' }]);
  assert.deepEqual(carousel(), ['t-elowen', 't-vesna-1', 't-bandit-1', 't-bandit-3']);

  const choices = Array.from(document.querySelectorAll('#combat-add-select option')).map((o) => (o as HTMLOptionElement).value);
  assert.ok(choices.includes('t-bandit-2'), 'the token that left is offered again');
  assert.ok(!choices.includes('t-elowen'), 'one already in the order is not');
  $<HTMLSelectElement>('#combat-add-select').value = 't-bandit-2';
  await click('#combat-add');
  assert.equal(fake.sent[1]?.dataCapability, 'table.addParticipant@1');
  assert.deepEqual(fake.sent[1]?.payload, { token: 't-bandit-2' }, 'an NPC is given no figure: the table rolls its d20');
  assert.ok(carousel().includes('t-bandit-2'));

  await click('#bm-end-combat');
  assert.equal(fake.sent[2]?.dataCapability, 'table.endCombat@1');
  assert.equal($('#initiative-carousel').hidden, true);
});

test('a PC added to the combat carries the initiative the DM enters', async () => {
  const { $, click, fake } = await battlemap({ view: 'dm' }, { combat: { ...underWay(), order: underWay().order.filter((o) => o.id !== 't-elowen') } });
  $<HTMLSelectElement>('#combat-add-select').value = 't-elowen';
  $<HTMLInputElement>('#combat-add-init').value = '16';
  await click('#combat-add');
  assert.deepEqual(fake.sent[0]?.payload, { token: 't-elowen', initiative: 16 });
});

test('the current participant removed hands the turn on first: table.nextTurn, then table.removeParticipant', async () => {
  const { $, click, open, fake, carousel } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await open('t-elowen');
  await click('#combat-drop');
  assert.deepEqual(fake.sent.map((c) => c.dataCapability), ['table.nextTurn@1', 'table.removeParticipant@1']);
  assert.deepEqual(fake.sent[1]?.payload, { participant: 't-elowen' });
  assert.ok($('#carousel-t-vesna-1').classList.contains('kit-token__item--current'));
  assert.ok(!carousel().includes('t-elowen'));
});

test('a participant that does not hold the turn is removed with no table.nextTurn before it', async () => {
  const { click, open, fake } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await open('t-bandit-1');
  await click('#combat-drop');
  assert.deepEqual(fake.sent.map((c) => c.dataCapability), ['table.removeParticipant@1']);
});

test('a token that holds the turn is taken off the map after table.nextTurn', async () => {
  const { $, click, open, fake } = await battlemap({ view: 'dm' }, { combat: underWay() });
  await open('t-elowen');
  await click('[data-writes="table.removeToken"]');
  assert.deepEqual(fake.sent.map((c) => c.dataCapability), ['table.nextTurn@1', 'table.removeToken@1']);
  assert.deepEqual(fake.sent[1]?.payload, { token: 't-elowen' });
  assert.ok($('#carousel-t-vesna-1').classList.contains('kit-token__item--current'));
});

test('« Réinitialiser la carte » asks first, then table.resetBattlemap empties the combat and restores the map', async () => {
  const { $, click, fake, document } = await battlemap({ view: 'dm' }, { combat: underWay(), tokens: (all) => all.slice(0, 3) });
  await click('#bm-reset');
  assert.equal(fake.sent.length, 0, 'a click only opens the Dialog');
  const dialog = $('#battlemap-reset-dialog [role="dialog"]');
  assert.equal(dialog.getAttribute('aria-label'), 'Réinitialiser la carte');
  assert.equal($('#battlemap-reset-submit').dataset.writes, 'table.resetBattlemap');
  await click('#battlemap-reset-cancel');
  assert.equal(document.querySelector('#battlemap-reset-dialog'), null);
  assert.equal(fake.sent.length, 0, 'cancelling resets nothing');
  await click('#bm-reset');
  await click('#battlemap-reset-submit');
  assert.deepEqual([fake.sent[0]?.dataCapability, fake.sent[0]?.payload], ['table.resetBattlemap@1', {}]);
  assert.equal(document.querySelector('#battlemap-reset-dialog'), null);
  assert.equal($('#initiative-carousel').hidden, true, 'the combat and its initiative are gone');
  assert.equal(document.querySelectorAll('#battlemap-tokens .kit-token__item').length, 8, 'the map reads what the table kept');
});

test('Escape and the close button leave a Dialog without sending', async () => {
  const { $, click, fake, document } = await battlemap({ view: 'dm' });
  await click('#bm-reset');
  $('#battlemap-reset-dialog').dispatchEvent(new document.defaultView!.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(document.querySelector('#battlemap-reset-dialog'), null);
  await click('#bm-start-combat');
  await click('[data-act="dialog-close"]');
  assert.equal(document.querySelector('#combat-start-dialog'), null);
  assert.equal(fake.sent.length, 0);
});

test('hidden-keeps-rank: the players’ carousel has no hidden token, and a token revealed again shows at its former rank', async () => {
  const dm = await battlemap({ view: 'dm' }, { combat: underWay() });
  assert.deepEqual(dm.carousel(), ['t-elowen', 't-vesna-1', 't-bandit-1', 't-bandit-2', 't-bandit-3']);
  assert.ok(dm.$('#carousel-t-vesna-1').classList.contains('kit-token__item--hidden'), 'the DM sees the hidden token marked');

  const players = await battlemap({ view: 'player' }, { combat: underWay() });
  assert.deepEqual(players.carousel(), ['t-elowen', 't-bandit-1', 't-bandit-2', 't-bandit-3']);
  assert.equal(players.document.querySelector('#carousel-t-vesna-1'), null);

  // The DM reveals the boss; the players read it again, second as before.
  const revealed = await battlemap({ view: 'player' }, { combat: underWay(), tokens: (all) => all.map((t) => (t.id === 't-vesna-1' ? { ...t, visible: true } : t)) });
  assert.deepEqual(revealed.carousel(), ['t-elowen', 't-vesna-1', 't-bandit-1', 't-bandit-2', 't-bandit-3']);
});

test('the player view’s carousel is read-only and shows no HP figure, only the band', async () => {
  const { document, $ } = await battlemap({ view: 'player' }, { combat: underWay() });
  assert.equal(document.querySelector('#carousel-controls'), null);
  assert.equal(document.querySelector('#bm-start-combat'), null);
  assert.equal(document.querySelector('#bm-reset'), null);
  assert.equal(document.querySelector('[draggable="true"]'), null);
  assert.equal($('#initiative-carousel').getAttribute('aria-label'), 'Ordre d’initiative');
  assert.doesNotMatch($('#initiative-carousel').textContent ?? '', /\d+\/\d+ PV/);
  assert.match($('#carousel-t-bandit-1').textContent ?? '', /Indemne/);
});

// AC1: a combat under way reads the same names and initiatives in the same order after a reopen
test('the carousel reads each participant’s initiative as the Capability answers it, before and after a reopen', async () => {
  const combat: Combat = { round: 1, current: 't-elowen', order: [{ id: 't-elowen', init: 18, dex: 0 }, { id: 't-sirielle', init: 14, dex: 0 }, { id: 't-bandit-2', init: 7, dex: 0 }] };
  const items = (document: Document): string[] =>
    Array.from(document.querySelectorAll('#carousel-track .kit-token__item')).map((el) => `${(el as HTMLElement).dataset.token}:${el.querySelector('.bm-init')?.textContent}`);
  const before = await battlemap({ view: 'dm' }, { combat, wire: true });
  const reopened = await battlemap({ view: 'dm' }, { combat, wire: true });
  assert.deepEqual(items(before.document), ['t-elowen:18', 't-sirielle:14', 't-bandit-2:7']);
  assert.deepEqual(items(reopened.document), items(before.document));
});

// AC1: the first .kit-token__item named Elowen is the map's token, the carousel being shown above it
test('during a combat the first token named Elowen is the map’s, and the carousel comes after the body in the page', async () => {
  const { $, document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  const named = Array.from(document.querySelectorAll<HTMLElement>('.kit-token__item')).filter((el) => el.textContent?.includes('Elowen'));
  assert.equal(named.length, 2);
  assert.equal(named[0]?.id, 'token-t-elowen');
  assert.ok(named[0]?.closest('#battlemap-map'));
  assert.ok(named[1]?.closest('#initiative-carousel'));
  assert.ok($('#battlemap-body').compareDocumentPosition($('#initiative-carousel')) & 4, 'the carousel follows the body');
  assert.match(STYLE, /\.bm-toolbar \{ order: -2;/);
  assert.match(STYLE, /\.bm-carousel \{ order: -1;/);
});

test('a refused combat command says why', async () => {
  const dm = await battlemap({ view: 'dm' }, { combat: underWay() });
  const refused: Shell = { ...dm.fake.shell, command: async () => ({ commandId: 'c', result: { status: 'rejected', violations: ['one-active-combat'] } }) };
  const dom = new JSDOM('<!doctype html><html><body><div data-component="Battlemap"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  mount(host, { instance: 'bmi-gue-s4', view: 'dm' }, refused, timing);
  await flush();
  (host.querySelector('#combat-next') as HTMLElement).click();
  await flush();
  assert.match(host.querySelector('#battlemap-status')?.textContent ?? '', /Le tour suivant refusé : il n’y a pas de combat en cours, ou il y en a déjà un/);
});

test('every actuator names its DataCapability and the carousel its Capability', async () => {
  const { document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  const writes = new Set(Array.from(document.querySelectorAll('[data-writes]')).map((el) => el.getAttribute('data-writes')));
  for (const unit of ['table.nextTurn', 'table.reorderInitiative', 'table.addParticipant', 'table.removeParticipant', 'table.endCombat', 'table.resetBattlemap']) assert.ok(writes.has(unit), `a part writes ${unit}`);
  assert.ok(document.querySelector('#battlemap[data-reads="table.getBattlemapInstance"]'));
});

test('the amount typed for one token does not follow the DM to another token’s sheet', async () => {
  const { $, pick, amount, click, fake } = await battlemap({ view: 'dm' });
  await pick('t-bandit-2');
  await amount('25');
  await pick('t-bandit-1');
  assert.equal($('#inspector-title').textContent, 'Bandit 1');
  assert.equal($<HTMLInputElement>('#inspector-hp-amount').value, '7', 'the new sheet starts from its own default');
  await click('#inspector-damage');
  assert.deepEqual(fake.sent[0]?.payload, { token: 't-bandit-1', delta: -7 });
});

test('a PC whose initiative is blank keeps the start dialog open and says who', async () => {
  const { $, click, fake, document } = await battlemap({ view: 'dm' });
  await click('#bm-start-combat');
  $<HTMLInputElement>('[data-init="t-kael"]').value = '';
  await click('#combat-start-submit');
  assert.equal(fake.sent.length, 0);
  assert.ok(document.querySelector('#combat-start-dialog'), 'the dialog stays, with its rolls');
  assert.equal($('#combat-start-error').hidden, false);
  assert.match($('#combat-start-error').textContent ?? '', /initiative de Kael/);
  $<HTMLInputElement>('[data-init="t-kael"]').value = '11';
  await click('#combat-start-submit');
  assert.equal((fake.sent[0]?.payload.initiative as Record<string, number>).kael, 11);
});

test('the rank a participant would take is marked while it is dragged over, and cleared when it drops', async () => {
  const { $, drag, document } = await battlemap({ view: 'dm' }, { combat: underWay() });
  const data: Record<string, string> = {};
  drag('dragstart', $('#carousel-t-bandit-3'), data);
  drag('dragover', $('#carousel-t-vesna-1'), data);
  assert.ok($('#carousel-t-vesna-1').classList.contains('bm-drag-over'));
  drag('dragover', $('#carousel-t-bandit-1'), data);
  assert.deepEqual(Array.from(document.querySelectorAll('.bm-drag-over')).map((el) => (el as HTMLElement).dataset.token), ['t-bandit-1']);
  drag('dragend', $('#carousel-t-bandit-3'), data);
  assert.equal(document.querySelectorAll('.bm-drag-over').length, 0);
});

test('a hidden token holding the turn is not named to the players, and nobody is highlighted', async () => {
  const { carousel, document } = await battlemap({ view: 'player' }, { combat: { ...underWay(), current: 't-vesna-1' } });
  assert.deepEqual(carousel(), ['t-elowen', 't-bandit-1', 't-bandit-2', 't-bandit-3']);
  assert.equal(document.querySelectorAll('.kit-token__item--current').length, 0);
});
