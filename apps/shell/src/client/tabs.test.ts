import assert from 'node:assert/strict';
import { test } from 'node:test';
import { wireTabs } from './tabs.js';

type Handler = () => void;

/** A tab or a panel, enough for `wireTabs`: attributes, `hidden`, listeners. */
function node(attrs: Record<string, string> = {}) {
  const listeners = new Map<string, Handler>();
  const set = new Map(Object.entries(attrs));
  return {
    hidden: false,
    dataset: { opens: attrs['data-opens'] },
    getAttribute: (name: string) => set.get(name) ?? null,
    setAttribute: (name: string, value: string) => void set.set(name, value),
    addEventListener: (type: string, handler: Handler) => void listeners.set(type, handler),
    fire: (type: string) => listeners.get(type)?.(),
    selected: () => set.get('aria-selected'),
  };
}

function page() {
  const tabs = [node({ 'aria-controls': 'p0', 'data-opens': 'fdm:open' }), node({ 'aria-controls': 'p1', 'data-opens': 'fdm:open-map' }), node({ 'aria-controls': 'p2' })];
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
  wireTabs(root as unknown as Document);
  return { tabs, panels, bus };
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
  bus.get('fdm:open-map')?.();
  assert.deepEqual(shown(panels), [false, true, false]);
  assert.equal(tabs[1]!.selected(), 'true');
  bus.get('fdm:open')?.();
  assert.deepEqual(shown(panels), [true, false, false]);
});

test('a tab with no event listens to no event', () => {
  const { bus } = page();
  assert.deepEqual([...bus.keys()].sort(), ['fdm:open', 'fdm:open-map']);
});
