import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, commandId, gridOf, initialForm, parseBattlemap, payload, problem, reasons, settled } from '../src/model.ts';

const answer = {
  id: 'bm-gue',
  title: 'Le Gué des Corbeaux',
  width: 960,
  height: 640,
  grid: { size: 30, ox: 12, oy: 8, orient: 'pointy', color: 'text', opacity: 0.4, feetPerHex: 5 },
  terrain: [{ tag: 'rect', class: 'hm-land', x: 0, y: 0, width: 960, height: 640 }, { tag: 'script', src: 'x' }, { tag: 'path', 'onload': 'x', d: 'M0 0' }, 'junk'],
};

test('the Capability answers a battlemap; no battlemap is null, and an answer that is not a readable one throws', () => {
  const map = parseBattlemap(answer);
  assert.equal(map?.title, 'Le Gué des Corbeaux');
  assert.deepEqual(map?.grid, { size: 30, ox: 12, oy: 8, orient: 'pointy', color: 'text', opacity: 0.4 });
  for (const none of [null, undefined]) assert.equal(parseBattlemap(none), null);
  for (const bad of ['x', [], { id: '' }, { id: 'a' }, { id: 'a', grid: 3 }]) assert.throws(() => parseBattlemap(bad));
});

test('a grid field that is missing or of another type is not replaced by a default a save would keep', () => {
  for (const field of ['size', 'ox', 'oy', 'orient', 'color', 'opacity']) {
    assert.throws(() => parseBattlemap({ ...answer, grid: { ...answer.grid, [field]: null } }), new RegExp(field), field);
  }
  assert.throws(() => parseBattlemap({ ...answer, grid: { ...answer.grid, opacity: '0.4' } }));
  assert.throws(() => parseBattlemap({ ...answer, width: 0 }));
  assert.throws(() => parseBattlemap({ ...answer, height: undefined }));
});

test('the opacity is kept to the hundredth of a percent: saving another field leaves it as it was', () => {
  for (const opacity of [0.455, 0.07, 0.004, 1, 0]) {
    const form = initialForm(parseBattlemap({ ...answer, grid: { ...answer.grid, opacity } })!);
    assert.equal(payload(form).opacity, opacity);
  }
});

test('only shapes the map may draw are kept, with plain attributes', () => {
  const map = parseBattlemap(answer);
  assert.deepEqual(map?.terrain.map((s) => s.tag), ['rect', 'path']);
  // Nothing that runs or fetches comes from data: `onload` is dropped, the geometry stays.
  assert.equal(map?.terrain[1]?.attrs.onload, undefined);
  const styled = parseBattlemap({ ...answer, terrain: [{ tag: 'rect', style: 'fill:url(//x)', href: '//x', fill: 'url(//x)', x: 1 }] });
  assert.deepEqual(styled?.terrain[0]?.attrs, { x: '1' });
  assert.equal(map?.terrain[1]?.attrs.d, 'M0 0');
  assert.equal(map?.terrain[0]?.attrs.class, 'hm-land');
  assert.equal(map?.terrain[0]?.attrs.width, '960');
});

test('the form shows the opacity as a percentage and the grid gives it back as a fraction of 1', () => {
  const form = initialForm(parseBattlemap(answer)!);
  assert.equal(form.opacity, 40);
  assert.equal(gridOf(form).opacity, 0.4);
  assert.equal(gridOf({ ...form, opacity: 20 }).opacity, 0.2);
});

test('a hex size of 0 or an opacity outside 0 to 100 % cannot be sent', () => {
  const form = initialForm(parseBattlemap(answer)!);
  assert.equal(problem(form), '');
  assert.equal(problem({ ...form, opacity: 0 }), '');
  assert.equal(problem({ ...form, opacity: 100 }), '');
  assert.match(problem({ ...form, size: 0 }), /taille/i);
  assert.match(problem({ ...form, size: -3 }), /taille/i);
  assert.match(problem({ ...form, opacity: 101 }), /opacité/i);
  assert.match(problem({ ...form, opacity: -1 }), /opacité/i);
  assert.match(problem({ ...form, size: Number.NaN }), /taille/i);
});

test('the payload is the six grid fields and nothing else, the opacity a fraction of 1', () => {
  const form = { ...initialForm(parseBattlemap(answer)!), size: 48, ox: 30, oy: 25, orient: 'flat' as const, color: 'success', opacity: 20 };
  assert.deepEqual(payload(form), { size: 48, ox: 30, oy: 25, orient: 'flat', color: 'success', opacity: 0.2 });
  assert.deepEqual(Object.keys(payload(form)).sort(), ['color', 'opacity', 'orient', 'ox', 'oy', 'size']);
});

test('the command names the DataCapability and the battlemap it targets, by identifier', () => {
  const body = commandBody('bm-gue', payload(initialForm(parseBattlemap(answer)!)));
  assert.deepEqual(body, {
    dataCapability: 'media.saveGridSettings@1',
    target: { aggregate: 'Media', id: 'bm-gue' },
    payload: { size: 30, ox: 12, oy: 8, orient: 'pointy', color: 'text', opacity: 0.4 },
    caller: { by: 'dm', from: 'GridSettings', roles: [] },
  });
});

test('a command answer is read as the server gives it', () => {
  assert.deepEqual(settled({ commandId: 'c1', result: { status: 'rejected', violations: ['grid-values-range', 3] } }), { status: 'rejected', violations: ['grid-values-range'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(commandId({ commandId: 'c1' }), 'c1');
  assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
  assert.match(reasons(['grid-values-range', 'unknown-one']), /opacité.*unknown-one/);
});
