import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));
const CAMPAIGN = fixture('campaign.getCampaign') as { id: string; name: string };

/** A 800 × 1200 illustration, as the library lists it. */
const MEDIA = { cover: 'ill-cover', illustrations: [{ id: 'ill-cover', title: 'Valombre', art: 'ember', glyph: 'V', width: 1600, height: 900 }, { id: 'ill-tall', title: 'Gaspard', art: 'gold', glyph: 'G', width: 800, height: 1200 }] };
const WINDOW = { media: 'ill-tall', x: 0.1, y: 0.1, w: 0.3 };
const SMALL = { media: 'ill-tall', x: 0.1, y: 0.1, w: 0.2 };
const STAGE = { width: 1000, height: 800 };

interface Sent {
  dataCapability: string;
  target: { aggregate: string };
  payload: Record<string, unknown>;
}

interface Options {
  diffusion?: unknown;
  media?: unknown;
  campaign?: unknown;
  /** What session.getLiveSession answers: the live session of the campaign, by default. */
  session?: unknown;
  /** What the server answers each command, in order; the last answer repeats. */
  status?: string | string[];
  /** Keeps every command waiting until it is released. */
  hold?: Promise<void>;
  violations?: string[];
  /** The data version the diffusion is read at, and the one the shell first tells. */
  readAt?: number;
  firstTold?: number;
  /** Makes the diffusion read throw while it answers true. */
  failRead?: () => boolean;
}

/** A shell over a one-row diffusion: reads answer it, a command applies to it as the DataGuard would. */
function fakeShell(options: Options = {}) {
  const row = structuredClone((options.diffusion ?? { base: { kind: 'cover', id: 'ill-cover' }, window: null, view: null }) as Record<string, unknown>);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables?: Record<string, unknown> }> = [];
  const listeners = new Set<(dataVersion: number) => void>();
  const told: Array<[string, unknown]> = [];
  const statusOf = (n: number): string => (Array.isArray(options.status) ? (options.status[n - 1] ?? options.status.at(-1)) : options.status) ?? 'applied';
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      if (id === 'live.getDiffusion' && options.failRead?.()) throw new Error('the server is not there');
      if (id === 'live.getDiffusion') return { asOf: options.readAt ?? 1, data: structuredClone(row) };
      if (id === 'session.getLiveSession') return { asOf: 1, data: 'session' in options ? options.session : { session: { id: 's4', campaign: CAMPAIGN.id }, microphone: { on: true } } };
      // The real read requires the campaign's id and has no answer without it.
      if (id === 'campaign.getCampaign') return { asOf: 1, data: variables?.id === undefined ? null : (options.campaign ?? { ...CAMPAIGN, id: variables.id }) };
      // The real read lists nothing for no campaign.
      return { asOf: 1, data: variables?.campaign === undefined ? { cover: '', illustrations: [] } : (options.media ?? MEDIA) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      await options.hold;
      const status = statusOf(sent.length);
      if (status === 'applied') Object.assign(row.window as object, command.dataCapability === 'live.moveWindow@1' ? { x: command.payload.x, y: command.payload.y } : { w: Math.min(command.payload.w as number, (command.payload.h as number) * (800 / 1200)) });
      return { commandId: `c${sent.length}`, result: { status, violations: options.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: statusOf(Number(commandId.slice(1))), violations: options.violations ?? [] } };
    },
    onDataVersion(listener) {
      listeners.add(listener);
      listener(options.firstTold ?? 1);
      return () => listeners.delete(listener);
    },
    context: { set: (name, value) => void told.push([name, value]) },
  };
  return { shell, sent, reads, row, told, moveOn: () => listeners.forEach((l) => l(2)) };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 3, watchMs: 1 };

const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function stage(props: Record<string, unknown>, options?: Options, url = 'http://localhost/') {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="PlayerStage"></div></body></html>', { url });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientWidth', { get: () => STAGE.width });
  Object.defineProperty(dom.window.HTMLElement.prototype, 'clientHeight', { get: () => STAGE.height });
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const composed: HTMLElement[] = [];
  const mounted = mount(host, props, fake.shell, timing, (box) => composed.push(box));
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the stage`);
    return found;
  };
  const pointer = (type: string, target: Element, x: number, y: number, button = 0): void => {
    target.dispatchEvent(new dom.window.MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button }));
  };
  const drag = async (target: Element, from: [number, number], to: [number, number], button = 0): Promise<void> => {
    pointer('pointerdown', target, ...from, button);
    pointer('pointermove', target, ...to);
    pointer('pointerup', target, ...to);
    await settle();
  };
  /** The window as drawn: width over height of its box, from the styles the stage gives it. */
  const drawn = () => {
    const figure = $('#player-window');
    const width = (parseFloat(figure.style.width) / 100) * STAGE.width;
    return { left: figure.style.left, top: figure.style.top, width: figure.style.width, ratio: width / (width / Number(figure.style.aspectRatio)) };
  };
  return { host, $, drag, drawn, composed, update: (next: Record<string, unknown>) => mounted.update(next), ...fake };
}

test('a diffusion on the cover shows the campaign’s cover and title', async () => {
  const s = await stage({});
  assert.equal(s.$('#player-cover-title').textContent, CAMPAIGN.name);
  assert.match(s.$('#player-cover').textContent ?? '', /Séance 4 — L’embuscade du Gué/);
  assert.ok(s.$('#player-cover [role="img"]'));
  assert.equal(s.$('#player-window').hidden, true);
  assert.deepEqual(s.reads.map((r) => r.id).sort(), ['campaign.getCampaign', 'live.getDiffusion', 'media.listMedia', 'session.getLiveSession']);
});

test('its parts name what they read and what they trigger', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  const named = (attribute: string): string[] => [...s.host.querySelectorAll(`[${attribute}]`)].map((e) => e.getAttribute(attribute) ?? '');
  assert.deepEqual([...new Set(named('data-reads'))].sort(), ['campaign.getCampaign', 'live.getDiffusion', 'media.listMedia', 'session.getLiveSession']);
  assert.deepEqual(named('data-writes').sort(), ['live.moveWindow', 'live.resizeWindow']);
});

test('an illustration sent as the scene replaces the cover, at its own ratio', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'illustration', id: 'ill-tall' }, window: null, view: null } });
  assert.equal(s.host.querySelector('#player-cover'), null);
  assert.match(s.$('#player-scene .pv-scene__art').getAttribute('style') ?? '', /aspect-ratio: 0\.666/);
  assert.ok(s.$('#player-scene [aria-label="Gaspard"]'));
});

test('a battlemap base leaves its slot to the Battlemap, in view player, and tells the screen', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'battlemap', id: 'inst-1' }, window: null, view: { zoom: 2, panX: 4, panY: 5 } } });
  const slot = s.$('#player-map');
  assert.equal(slot.dataset.component, 'Battlemap');
  assert.deepEqual(JSON.parse(slot.dataset.props ?? '{}'), { instance: 'inst-1', view: 'player', pilot: true, fill: true });
  assert.deepEqual([slot.dataset.zoom, slot.dataset.panX, slot.dataset.panY], ['2', '4', '5']);
  assert.equal(s.composed.length, 1);
  // The same row again does not mount the map a second time.
  s.moveOn();
  await settle();
  assert.equal(s.composed.length, 1);
  assert.deepEqual(s.told.at(-1), ['battlemap', 'inst-1']);
});

test('the DM drags the corner to a square: live.resizeWindow is submitted and the window keeps 2/3', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: SMALL, view: null } });
  assert.ok(Math.abs(s.drawn().ratio - 2 / 3) < 1e-9, 'before the drag');
  // 200 × 300 px window, its corner dragged to a 400 px square.
  await s.drag(s.$('#player-window-resize'), [300, 400], [500, 500]);
  assert.equal(s.sent.length, 1);
  assert.equal(s.sent[0]?.dataCapability, 'live.resizeWindow@1');
  assert.deepEqual(s.sent[0]?.target, { aggregate: 'Diffusion' });
  assert.equal(s.sent[0]?.payload.campaign, CAMPAIGN.id);
  assert.equal(s.sent[0]?.payload.w, s.sent[0]?.payload.h, 'the DM drew a square');
  assert.ok(Math.abs(parseFloat(s.drawn().width) - 100 * 0.4 * (2 / 3)) < 0.01, 'the window is the largest of the image’s ratio the square holds');
  assert.ok(Math.abs(s.drawn().ratio - 2 / 3) < 1e-9, 'after the drag');
  assert.equal(s.sent.some((c) => c.dataCapability === 'live.moveWindow@1'), false);
});

test('the DM drags the bar: live.moveWindow is submitted with the new x, y', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  await s.drag(s.$('#player-window-bar'), [200, 100], [300, 180]);
  assert.equal(s.sent.length, 1);
  assert.equal(s.sent[0]?.dataCapability, 'live.moveWindow@1');
  assert.deepEqual(s.sent[0]?.payload, { campaign: CAMPAIGN.id, x: 0.2, y: 0.2 });
  assert.equal(s.drawn().left, '20%');
  assert.equal(s.drawn().top, '20%');
  assert.equal(s.drawn().width, '30%');
});

test('a click on the bar without a drag sends nothing', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  await s.drag(s.$('#player-window-bar'), [200, 100], [200, 100]);
  assert.deepEqual(s.sent, []);
});

test('the projected screen has a corner but no bar, and draws the window where the DM’s tab does', async () => {
  const diffusion = { base: { kind: 'cover' }, window: WINDOW, view: null };
  const projected = await stage({}, { diffusion });
  assert.equal(projected.host.querySelector('#player-window-bar'), null);
  assert.ok(projected.$('#player-window-resize'));
  assert.equal(projected.host.querySelector('#player-pilot-badge'), null);
  await projected.drag(projected.$('#player-window'), [200, 100], [400, 300]);
  assert.deepEqual(projected.sent, []);
  const pilot = await stage({ pilot: true }, { diffusion });
  assert.deepEqual(projected.drawn(), pilot.drawn());
  assert.deepEqual(pilot.reads.find((r) => r.id === 'live.getDiffusion')?.variables, { view: 'dm' });
  assert.deepEqual(projected.reads.find((r) => r.id === 'live.getDiffusion')?.variables, { view: 'player' });
});

// AC1: the corner of the projected window, dragged, widens it and keeps the illustration's ratio
test('the projected screen’s corner resizes the window through live.resizeWindow, ratio kept', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: SMALL, view: null } });
  const before = parseFloat(s.drawn().width);
  // 200 × 300 px window, its corner dragged by +120, +40.
  await s.drag(s.$('#player-window-resize'), [300, 400], [420, 440]);
  assert.equal(s.sent.length, 1);
  assert.equal(s.sent[0]?.dataCapability, 'live.resizeWindow@1');
  assert.equal(s.sent[0]?.payload.campaign, CAMPAIGN.id);
  assert.ok(parseFloat(s.drawn().width) > before, 'the window is wider');
  assert.ok(Math.abs(s.drawn().ratio - 2 / 3) < 0.1, 'the image’s ratio is kept');
  assert.deepEqual(s.reads.find((r) => r.id === 'live.getDiffusion')?.variables, { view: 'player' });
});

test('the projected screen’s bar-less window cannot be moved, only resized', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  await s.drag(s.$('#player-window-art'), [200, 100], [300, 180]);
  assert.deepEqual(s.sent, []);
});

test('?pilote=1 pilots the tab without the prop', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } }, 'http://localhost/screens/player-view?pilote=1');
  assert.ok(s.$('#player-window-bar'));
});

test('the projected screen follows the row when it moves', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  Object.assign(s.row.window as object, { x: 0.5, w: 0.2 });
  s.moveOn();
  await settle();
  assert.equal(s.drawn().left, '50%');
  assert.equal(s.drawn().width, '20%');
  s.row.window = null;
  s.moveOn();
  await settle();
  assert.equal(s.$('#player-window').hidden, true);
});

test('a refused move puts the window back where the row has it and says why', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, status: 'rejected', violations: ['window-fields-together'] });
  await s.drag(s.$('#player-window-bar'), [200, 100], [300, 180]);
  assert.equal(s.drawn().left, '10%');
  assert.match(s.$('#player-status').textContent ?? '', /aucune fenêtre n’est ouverte/);
});

test('no HP figure, note or hidden token is ever drawn, whatever the answers carry', async () => {
  for (const base of [{ kind: 'cover' }, { kind: 'illustration', id: 'ill-tall' }, { kind: 'battlemap', id: 'inst-1' }]) {
    const diffusion = {
      base: { ...base, hp: 17, notes: 'LE-SECRET' },
      window: { ...WINDOW, notes: 'LE-SECRET' },
      view: null,
      tokens: [{ name: 'JeSuisCaché', visible: false, hp: 17 }],
      hp: 17,
      notes: 'LE-SECRET',
    };
    const campaign = { ...(CAMPAIGN as object), notes: 'LE-SECRET', hp: 17 };
    for (const pilot of [false, true]) {
      const s = await stage({ pilot }, { diffusion, campaign, media: { ...MEDIA, notes: 'LE-SECRET', illustrations: MEDIA.illustrations.map((i) => ({ ...i, notes: 'LE-SECRET' })) } });
      assert.doesNotMatch(s.host.innerHTML, /LE-SECRET|JeSuisCaché|17|\bPV\b/, `${base.kind}, pilot ${pilot}`);
      assert.equal(s.host.querySelector('.kit-token__item'), null);
    }
  }
});

test('a right click on the bar starts no drag', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  await s.drag(s.$('#player-window-bar'), [200, 100], [300, 180], 2);
  assert.deepEqual(s.sent, []);
  assert.equal(s.drawn().left, '10%');
});

test('without its campaign the window is not modified, and the DM is told', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, campaign: {} });
  await s.drag(s.$('#player-window-bar'), [200, 100], [300, 180]);
  assert.deepEqual(s.sent, []);
  assert.match(s.$('#player-status').textContent ?? '', /campagne/);
  assert.equal(s.drawn().left, '10%');
});

test('a change that came between the read and the first version told is not missed', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, readAt: 1, firstTold: 2 });
  assert.equal(s.reads.filter((r) => r.id === 'live.getDiffusion').length, 2);
  const quiet = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, readAt: 3, firstTold: 3 });
  assert.equal(quiet.reads.filter((r) => r.id === 'live.getDiffusion').length, 1);
});

test('an image the library does not list is asked for once, not at every change', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: { ...WINDOW, media: 'ill-gone' }, view: null } });
  s.moveOn();
  await settle();
  s.moveOn();
  await settle();
  assert.equal(s.reads.filter((r) => r.id === 'media.listMedia').length, 1);
  assert.equal(s.$('#player-window').hidden, true);
});

test('a restart while a command is under way leaves the new stage’s count alone', async () => {
  let release = (): void => undefined;
  const hold = new Promise<void>((resolve) => (release = resolve));
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, status: ['applied', 'rejected'], violations: ['window-fields-together'], hold });
  await s.drag(s.$('#player-window-bar'), [200, 100], [300, 180]);
  // The stage starts anew while the first command waits, then the command settles.
  s.update({ pilot: false });
  s.update({ pilot: true });
  release();
  await settle();
  // The next drag is refused: the window goes back where the row has it.
  await s.drag(s.$('#player-window-bar'), [200, 100], [400, 180]);
  assert.equal(s.sent.length, 2);
  assert.equal(s.drawn().left, '20%', 'the row holds the first, applied move');
  assert.match(s.$('#player-status').textContent ?? '', /aucune fenêtre/);
});

test('a campaign named by the screen is the one every read and every command names', async () => {
  const s = await stage({ pilot: true, campaign: 'c-9' }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, campaign: {} });
  const variables = (id: string) => s.reads.find((r) => r.id === id)?.variables;
  assert.deepEqual(variables('live.getDiffusion'), { view: 'dm', campaign: 'c-9' });
  assert.deepEqual(variables('campaign.getCampaign'), { id: 'c-9' });
  assert.deepEqual(variables('media.listMedia'), { campaign: 'c-9' });
  // The campaign's own answer names none, yet the window moves: the screen's campaign is enough.
  await s.drag(s.$('#player-window-bar'), [200, 200], [300, 260]);
  assert.equal(s.sent[0]?.payload.campaign, 'c-9');
});

// AC1: no campaign prop, nothing sent — the live campaign's title is the h1 and its cover is drawn
test('a screen that names no campaign leaves the server\u2019s live session to decide', async () => {
  const s = await stage({}, { session: { session: { id: 's4', campaign: 'c-live' }, microphone: { on: true } } });
  assert.deepEqual(s.reads.find((r) => r.id === 'live.getDiffusion')?.variables, { view: 'player' });
  assert.deepEqual(s.reads.find((r) => r.id === 'session.getLiveSession')?.variables, {});
  assert.deepEqual(s.reads.find((r) => r.id === 'campaign.getCampaign')?.variables, { id: 'c-live' });
  assert.deepEqual(s.reads.find((r) => r.id === 'media.listMedia')?.variables, { campaign: 'c-live' });
  assert.equal(s.$('#player-cover h1').textContent, CAMPAIGN.name);
  assert.ok(s.$('#player-cover [role="img"]'));
});

// AC1: no live session, no campaign to show — campaign.getCampaign is never asked without its id
test('with no live session the stage asks no campaign and draws no title', async () => {
  const s = await stage({}, { session: null });
  assert.equal(s.reads.filter((r) => r.id === 'campaign.getCampaign').length, 0);
  assert.equal(s.host.querySelector('h1'), null);
});

// AC1: the live session's campaign lists the library, so the window the DM opened shows its illustration
test('a window on a stage with no campaign prop shows the illustration of the live campaign’s library', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  assert.deepEqual(s.reads.find((r) => r.id === 'media.listMedia')?.variables, { campaign: CAMPAIGN.id });
  assert.equal(s.$('#player-window').hidden, false);
  assert.ok(s.$('#player-window [aria-label="Gaspard"]'));
});

// AC2: the window of a stage with no campaign prop is resized from the pilot tab and keeps the image's ratio
test('the corner resizes a window found through the live campaign’s library, ratio kept', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  // 300 × 450 px window at (100, 80), its corner dragged to a 600 px square.
  await s.drag(s.$('#player-window-resize'), [400, 530], [700, 630]);
  assert.equal(s.sent[0]?.dataCapability, 'live.resizeWindow@1');
  assert.ok(Math.abs(s.drawn().ratio - 2 / 3) < 1e-9);
  assert.ok(parseFloat(s.drawn().width) > 30, 'the window grew');
});

// AC3: with no live session and no campaign prop, no media read is made
test('with no live session the library is never asked without a campaign', async () => {
  const s = await stage({}, { session: null });
  assert.equal(s.reads.filter((r) => r.id === 'media.listMedia').length, 0);
});

// AC2: the window commands name the live campaign when the screen gives no campaign prop
test('a window moved on a stage with no campaign prop names the live session’s campaign', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, session: { session: { id: 's4', campaign: 'c-live' } } });
  await s.drag(s.$('#player-window-bar'), [200, 200], [300, 260]);
  assert.equal(s.sent[0]?.payload.campaign, 'c-live');
  assert.equal(s.$('#player-status').hidden, true);
});

test('the DM is told when the diffusion cannot be read, and the next good read takes it down', async () => {
  let down = true;
  const s = await stage({ pilot: true }, { failRead: () => down });
  assert.equal(s.$('#player-status').hidden, false);
  assert.match(s.$('#player-status').textContent ?? '', /Impossible de lire la diffusion/);
  down = false;
  s.moveOn();
  await settle();
  assert.equal(s.$('#player-status').hidden, true);
});

test('a refused move keeps its message through the read that follows it', async () => {
  const s = await stage({ pilot: true }, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null }, status: 'rejected', violations: ['window-fields-together'] });
  await s.drag(s.$('#player-window-bar'), [200, 200], [300, 260]);
  assert.match(s.$('#player-status').textContent ?? '', /aucune fenêtre n’est ouverte/);
  s.moveOn();
  await settle();
  assert.equal(s.$('#player-status').hidden, false);
});

test('a new campaign from the screen starts the stage over on it', async () => {
  const s = await stage({ campaign: 'c-1' });
  s.update({ campaign: 'c-2' });
  await settle();
  const last = (id: string) => s.reads.filter((r) => r.id === id).at(-1)?.variables;
  assert.deepEqual(last('live.getDiffusion'), { view: 'player', campaign: 'c-2' });
  assert.deepEqual(last('campaign.getCampaign'), { id: 'c-2' });
  assert.deepEqual(last('media.listMedia'), { campaign: 'c-2' });
});

// AC1: a window shown takes the cover's title off the stage; the figure keeps its illustration, and the title comes back with the window gone
test('a floating window takes the cover’s title off the stage, and the title returns when it closes', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: WINDOW, view: null } });
  assert.equal(s.$('#player-window').hidden, false);
  assert.ok(s.$('#player-window [aria-label="Gaspard"]'));
  assert.equal(s.host.querySelector('h1'), null);
  assert.equal(s.host.querySelector('#player-cover-title'), null);
  assert.doesNotMatch(s.$('#player-cover').textContent ?? '', /Séance 4/);
  s.row.window = null;
  s.moveOn();
  await settle();
  assert.equal(s.$('#player-cover-title').textContent, CAMPAIGN.name);
});

// AC1: a window whose image the library does not list is not drawn, so it takes nothing off the stage
test('a window the library does not list leaves the cover’s title', async () => {
  const s = await stage({}, { diffusion: { base: { kind: 'cover' }, window: { ...WINDOW, media: 'ill-gone' }, view: null } });
  assert.equal(s.$('#player-window').hidden, true);
  assert.equal(s.$('#player-cover-title').textContent, CAMPAIGN.name);
});

// AC2: no window, the player view opens on the cover's level-1 heading and image
test('with no window the cover’s level-1 heading and image are drawn', async () => {
  const s = await stage({});
  assert.equal(s.$('#player-cover h1').textContent, CAMPAIGN.name);
  assert.ok(s.$('#player-cover [role="img"]'));
  assert.equal(s.$('#player-window').hidden, true);
});

// AC1: the cover is on the stage, then the DM sends the window — the title goes off the stage without a restart
test('a window sent over the shown cover takes its title off the stage', async () => {
  const s = await stage({});
  assert.equal(s.$('#player-cover h1').textContent, CAMPAIGN.name);
  s.row.window = { ...WINDOW };
  s.moveOn();
  await settle();
  assert.equal(s.$('#player-window').hidden, false);
  assert.equal(s.host.querySelector('h1'), null);
  assert.ok(s.$('#player-cover [role="img"]'));
});
