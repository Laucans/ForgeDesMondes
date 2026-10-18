import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

/** The mock-up's fixture of the Capability, as the Capability answers it: the opacity a fraction of 1, a hex 5 feet. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/media.getBattlemap.json', import.meta.url), 'utf8')) as {
  id: string;
  grid: { opacity: number; size: number } & Record<string, unknown>;
} & Record<string, unknown>;
const battlemap = (grid: Record<string, unknown> = {}) => ({ ...fixture, grid: { ...fixture.grid, opacity: fixture.grid.opacity / 100, feetPerHex: 5, ...grid } });

interface Sent {
  dataCapability: string;
  target: { aggregate: string; id: string };
  payload: Record<string, unknown>;
}
interface Options {
  status?: string;
  read?: (id: string) => unknown;
  apply?: (sent: Sent) => { status: string; violations?: string[] };
  readAfterSave?: () => unknown;
}

function fakeShell(options: Options = {}) {
  const sent: Sent[] = [];
  const reads: string[] = [];
  let saved = false;
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(id, 'media.getBattlemap');
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push(String(variables?.id));
      if (saved && options.readAfterSave) return { asOf: 2, data: options.readAfterSave() };
      return { asOf: 1, data: options.read ? options.read(String(variables?.id)) : variables?.id === fixture.id ? battlemap() : null };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = options.apply ? options.apply(command) : { status: options.status ?? 'applied' };
      saved = true;
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: options.status ?? 'applied', violations: [] } };
    },
  };
  return { shell, sent, reads };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 3 };
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function panel(options?: Options, props: Record<string, unknown> = { battlemap: fixture.id }) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="GridSettings"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the panel`);
    return found;
  };
  const type = (selector: string, value: string): void => {
    const field = $(selector) as HTMLInputElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  const choose = (selector: string, value?: string): void => {
    const field = $(selector) as HTMLInputElement | HTMLSelectElement;
    if (value === undefined) (field as HTMLInputElement).checked = true;
    else field.value = value;
    field.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const look = (): string => {
    const path = $('.hm-grid-path');
    return [path.getAttribute('d'), path.style.opacity, $('.kit-hexmap').style.getPropertyValue('--hm-grid')].join('|');
  };
  return { host, mounted, $, type, choose, click, look, text: () => host.textContent ?? '', ...fake };
}

test('the panel opens on the battlemap, its grid drawn, and says one hex is 5 feet', async () => {
  const p = await panel();
  assert.deepEqual(p.reads, ['bm-gue']);
  assert.match(p.$('#grid-title').textContent ?? '', /Le Gué des Corbeaux/);
  assert.match(p.$('#grid-scale').textContent ?? '', /1 hex = 5 pi/);
  assert.ok(($('d') ?? '').length > 0);
  function $(attr: string): string | null {
    return p.$('.hm-grid-path').getAttribute(attr);
  }
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '30');
  assert.equal((p.$('#grid-opacity') as HTMLInputElement).value, '40');
  assert.equal(p.$('.hm-grid-path').style.opacity, '0.4');
  assert.equal(p.$('.kit-hexmap').style.getPropertyValue('--hm-grid'), 'var(--color-text)');
  assert.equal((p.$('#grid-save') as HTMLButtonElement).disabled, false);
});

test('every setting redraws the grid at once, and the 5 feet stay', async () => {
  const p = await panel();
  const changes: Array<() => void> = [
    () => p.type('#grid-size', '48'),
    () => p.type('#grid-ox', '30'),
    () => p.type('#grid-oy', '25'),
    () => p.choose('input[name="grid-orient"][value="flat"]'),
    () => p.choose('#grid-color', 'success'),
    () => p.type('#grid-opacity', '20'),
  ];
  for (const change of changes) {
    const before = p.look();
    change();
    assert.notEqual(p.look(), before);
    assert.match(p.$('#grid-scale').textContent ?? '', /1 hex = 5 pi/);
  }
  assert.equal(p.$('.kit-hexmap').style.getPropertyValue('--hm-grid'), 'var(--color-success)');
  assert.equal(p.$('.hm-grid-path').style.opacity, '0.2');
  assert.equal(p.$('#grid-opacity').closest('.kit-slider__field')?.querySelector('.kit-slider__out')?.textContent, '20');
  assert.equal(p.sent.length, 0, 'nothing is sent while the DM only looks');
});

test('« Enregistrer » sends the six grid fields to media.saveGridSettings, the opacity a fraction of 1', async () => {
  const p = await panel();
  p.type('#grid-size', '48');
  p.type('#grid-ox', '30');
  p.type('#grid-oy', '25');
  p.choose('input[name="grid-orient"][value="flat"]');
  p.choose('#grid-color', 'success');
  p.type('#grid-opacity', '20');
  await p.click('#grid-save');
  assert.equal(p.sent.length, 1);
  assert.equal(p.sent[0]?.dataCapability, 'media.saveGridSettings@1');
  assert.deepEqual(p.sent[0]?.target, { aggregate: 'Media', id: 'bm-gue' });
  assert.deepEqual(p.sent[0]?.payload, { size: 48, ox: 30, oy: 25, orient: 'flat', color: 'success', opacity: 0.2 });
  assert.match(p.text(), /Réglage enregistré/);
});

test('a hex size of 0 is not sent', async () => {
  const p = await panel({ read: () => battlemap({ size: 0 }) });
  await p.click('#grid-save');
  assert.equal(p.sent.length, 0);
  assert.match(p.$('#grid-message').textContent ?? '', /taille/i);
});

test('an opacity outside 0 to 100 % is not sent', async () => {
  const p = await panel({ read: () => battlemap({ opacity: 1.5 }) });
  await p.click('#grid-save');
  assert.equal(p.sent.length, 0);
  assert.match(p.$('#grid-message').textContent ?? '', /opacité/i);
});

test('a refused save says why, and keeps what the DM typed', async () => {
  const p = await panel({ apply: () => ({ status: 'rejected', violations: ['grid-values-range'] }) });
  p.type('#grid-size', '48');
  await p.click('#grid-save');
  assert.match(p.$('#grid-message').textContent ?? '', /refusé.*taille/i);
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '48');
  assert.equal((p.$('#grid-save') as HTMLButtonElement).disabled, false, 'the DM can try again');
});

test('« Annuler les changements » brings back the saved grid', async () => {
  const p = await panel();
  const before = p.look();
  p.type('#grid-size', '48');
  assert.notEqual(p.look(), before);
  await p.click('#grid-reset');
  assert.equal(p.look(), before);
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '30');
});

test('after a save, « Annuler » goes back to what was saved, as the Capability now answers it', async () => {
  const p = await panel({ readAfterSave: () => battlemap({ size: 48 }) });
  p.type('#grid-size', '48');
  await p.click('#grid-save');
  p.type('#grid-size', '20');
  await p.click('#grid-reset');
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '48');
});

test('no battlemap named, an unknown one, or an unreadable one: a notice, nothing to save', async () => {
  const none = await panel(undefined, {});
  assert.match(none.$('#grid-notice').textContent ?? '', /Choisissez/);
  assert.equal((none.$('#grid-save') as HTMLButtonElement).disabled, true);
  assert.match(none.$('#grid-scale').textContent ?? '', /1 hex = 5 pi/, 'the scale is there in any state');

  const unknown = await panel(undefined, { battlemap: 'ill-valombre' });
  assert.match(unknown.$('#grid-notice').textContent ?? '', /n’existe pas/);

  const broken = await panel({ read: () => { throw new Error('down'); } });
  assert.match(broken.$('#grid-notice').textContent ?? '', /Impossible de lire/);
  assert.equal((broken.$('#grid-save') as HTMLButtonElement).disabled, true);
});

test('the screen can hand it another battlemap, and an answer that comes late is dropped', async () => {
  const p = await panel({ read: (id) => (id === 'bm-gue' ? battlemap() : { ...battlemap({ size: 34 }), id: 'bm-moulin', title: 'Les ruines du moulin' }) });
  p.mounted.update({ battlemap: 'bm-moulin' });
  await settle();
  assert.match(p.$('#grid-title').textContent ?? '', /Les ruines du moulin/);
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '34');
  const before = p.reads.length;
  p.mounted.update({ battlemap: 'bm-moulin' });
  await settle();
  assert.equal(p.reads.length, before, 'the same battlemap is not read again');
});

test('the parts name what they read and write, by identifier', async () => {
  const p = await panel();
  assert.ok(p.host.querySelector('[data-reads="media.getBattlemap"]'));
  assert.equal(p.$('#grid-save').dataset.writes, 'media.saveGridSettings');
});

test('a save still queued or parked when the panel stops asking is said to be waiting, not failed', async () => {
  const p = await panel({ status: 'parked' });
  await p.click('#grid-save');
  assert.match(p.$('.mf-grid__result').textContent ?? '', /en attente \(état : parked\)/);
  assert.equal((p.$('#grid-save') as HTMLButtonElement).disabled, false);
});

test('a save that ends after the screen handed the panel another battlemap says nothing of it and keeps the new one', async () => {
  const other = { ...battlemap({ size: 34 }), id: 'bm-moulin', title: 'Les ruines du moulin' };
  const p = await panel({ read: (id) => (id === 'bm-gue' ? battlemap() : other), apply: () => ({ status: 'applied' }) }, { battlemap: 'bm-gue' });
  const shell = p.mounted;
  // The screen moves on before the server has answered the command.
  const original = p.$('#grid-save');
  original.dispatchEvent(new (original.ownerDocument.defaultView as Window & typeof globalThis).MouseEvent('click', { bubbles: true }));
  shell.update({ battlemap: 'bm-moulin' });
  await settle();
  assert.match(p.$('#grid-title').textContent ?? '', /Les ruines du moulin/);
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '34');
  assert.doesNotMatch(p.$('.mf-grid__result').textContent ?? '', /enregistré/);
  await p.click('#grid-reset');
  assert.equal((p.$('#grid-size') as HTMLInputElement).value, '34', 'the baseline is the new battlemap, not the one that was saved');
});

test('an answer that is not a battlemap is unreadable, not an empty one', async () => {
  const p = await panel({ read: () => ({ id: 'bm-gue', grid: { size: null } }) });
  assert.match(p.$('#grid-notice').textContent ?? '', /Impossible de lire/);
});
