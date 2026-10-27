import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Context } from './context.js';
import { wireTabs } from './tabs.js';

type Handler = (event?: unknown) => void;

/** A tab or a panel, enough for `wireTabs`: attributes, `hidden`, listeners. */
function node(attrs: Record<string, string> = {}) {
  const listeners = new Map<string, Handler>();
  const set = new Map(Object.entries(attrs));
  return {
    hidden: false,
    dataset: { opens: attrs['data-opens'], sets: attrs['data-sets'] },
    getAttribute: (name: string) => set.get(name) ?? null,
    setAttribute: (name: string, value: string) => void set.set(name, value),
    addEventListener: (type: string, handler: Handler) => void listeners.set(type, handler),
    fire: (type: string) => listeners.get(type)?.(),
    selected: () => set.get('aria-selected'),
  };
}

function page() {
  const tabs = [
    node({ 'aria-controls': 'p0', 'data-opens': 'fdm:open' }),
    node({ 'aria-controls': 'p1', 'data-opens': 'fdm:open-map', 'data-sets': 'battlemap:id' }),
    node({ 'aria-controls': 'p2' }),
  ];
  const panels = [node(), node(), node()];
  panels[1]!.hidden = true;
  panels[2]!.hidden = true;
  const bus = new Map<string, Handler>();
  const group = { querySelectorAll: () => tabs };
  const root = {
    querySelectorAll: () => [group],
    getElementById: (id: string) => panels[Number(id.slice(1))] ?? null,
    addEventListener: (type: string, handler: Handler) => void bus.set(type, handler),
  };
  const context = new Context();
  wireTabs(root as unknown as Document, context);
  return { tabs, panels, bus, context };
}

const shown = (panels: Array<{ hidden: boolean }>): boolean[] => panels.map((panel) => !panel.hidden);

// AC10: a tab brings its own panel forward and hides the others, so the centre switches without remounting anything
test('a click on a tab shows its panel only', () => {
  const { tabs, panels } = page();
  tabs[2]!.fire('click');
  assert.deepEqual(shown(panels), [false, false, true]);
  assert.deepEqual(tabs.map((tab) => tab.selected()), ['false', 'false', 'true']);
});

// AC2, AC11: an element opened from the panel, the sheet or the pill brings the sheet forward; the map asked for brings the battlemap
test('the events on the page bring the sheet or the battlemap forward', () => {
  const { tabs, panels, bus } = page();
  bus.get('fdm:open-map')?.({ detail: {} });
  assert.deepEqual(shown(panels), [false, true, false]);
  assert.equal(tabs[1]!.selected(), 'true');
  bus.get('fdm:open')?.({ detail: { type: 'npc', id: 'vesna' } });
  assert.deepEqual(shown(panels), [true, false, false]);
});

test('a tab with no event listens to no event', () => {
  const { bus } = page();
  assert.deepEqual([...bus.keys()].sort(), ['fdm:open', 'fdm:open-map']);
});

// AC2: the battlemap asked for by instance becomes the one the Battlemap follows, and the tab is shown with it
test('fdm:open-map sets the battlemap context from the event detail', () => {
  const { panels, bus, context } = page();
  const heard: string[] = [];
  context.on((name, value) => heard.push(`${name}=${String(value)}`));
  bus.get('fdm:open-map')?.({ detail: { id: 'bmi-gue-s4' } });
  assert.equal(context.get('battlemap'), 'bmi-gue-s4');
  assert.deepEqual(heard, ['battlemap=bmi-gue-s4']);
  assert.deepEqual(shown(panels), [false, true, false]);
});

test('an open-map with no instance shows the battlemap and keeps the one it had', () => {
  const { panels, bus, context } = page();
  bus.get('fdm:open-map')?.({ detail: { id: 'bmi-gue-s4' } });
  bus.get('fdm:open')?.({ detail: { type: 'npc', id: 'vesna' } });
  bus.get('fdm:open-map')?.({ detail: {} });
  bus.get('fdm:open-map')?.({ detail: { place: 'tavern' } });
  bus.get('fdm:open-map')?.({ detail: null });
  assert.equal(context.get('battlemap'), 'bmi-gue-s4');
  assert.deepEqual(shown(panels), [false, true, false]);
});

// AC11: the sheet opened by an element (the pill's history, the panel) leaves the context alone
test('fdm:open brings the sheet forward and sets no context', () => {
  const { panels, bus, context } = page();
  bus.get('fdm:open-map')?.({ detail: { id: 'bmi-gue-s4' } });
  bus.get('fdm:open')?.({ detail: { type: 'place', id: 'tavern' } });
  assert.deepEqual(shown(panels), [true, false, false]);
  assert.equal(context.get('battlemap'), 'bmi-gue-s4');
});

test('a click on the battlemap tab sets no context', () => {
  const { tabs, context } = page();
  tabs[1]!.fire('click');
  assert.equal(context.get('battlemap'), undefined);
});
