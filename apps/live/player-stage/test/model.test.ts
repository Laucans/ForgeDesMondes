import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, dragged, heightOf, moved, parseCampaign, parseDiffusion, parseMedia, parseProps, resized, settled, widest } from '../src/model.ts';

const stage = { width: 1000, height: 800 };

test('no diffusion row is the cover alone', () => {
  assert.deepEqual(parseDiffusion(undefined), { base: { kind: 'cover', id: '' }, window: null, view: null });
  assert.deepEqual(parseDiffusion({ base: { kind: 'nonsense' }, window: { media: 'a' } }), { base: { kind: 'cover', id: '' }, window: null, view: null });
});

test('the diffusion is read as the Capability answers it, and as the mock-up stored it', () => {
  const real = parseDiffusion({ base: { kind: 'battlemap', id: 'bm' }, window: { media: 'ill', x: 0.1, y: 0.2, w: 0.3, h: 0.4 }, view: { zoom: 2, panX: 5, panY: -6 } });
  assert.deepEqual(real.window, { media: 'ill', x: 0.1, y: 0.2, w: 0.3 });
  assert.deepEqual(real.view, { zoom: 2, panX: 5, panY: -6 });
  const stored = parseDiffusion({ base: { kind: 'illustration', id: 'ill' }, window: { id: 'ill', x: 0.6, y: 0.12, w: 0.3 }, view: { z: 1.5, px: 1, py: 2 } });
  assert.deepEqual(stored.window, { media: 'ill', x: 0.6, y: 0.12, w: 0.3 });
  assert.deepEqual(stored.view, { zoom: 1.5, panX: 1, panY: 2 });
});

test('only what a player may see is kept of an answer', () => {
  const answer = { base: { kind: 'battlemap', id: 'bm', hp: 12, notes: 'secret' }, window: null, view: null, tokens: [{ name: 'Hidden', visible: false }], notes: 'n', hp: 3 };
  assert.equal(JSON.stringify(parseDiffusion(answer)).match(/secret|Hidden|hp|notes/), null);
  const campaign = parseCampaign({ id: 'c', name: 'Valombre', notes: 'secret', hp: 4, nextSession: { order: 4, title: 'Gué', notes: 'secret' } });
  assert.equal(JSON.stringify(campaign).match(/secret|hp|notes/), null);
});

test('the image gives the ratio: its pixels first, then the ratio it states', () => {
  const media = parseMedia({ illustrations: [{ id: 'a', width: 800, height: 1200, ratio: 9 }, { id: 'b', ratio: 1.78 }, { id: 'c' }, { title: 'no id' }] });
  assert.deepEqual(media.illustrations.map((i) => i.ratio), [800 / 1200, 1.78, 1]);
});

test('the pilot tab is asked for by the prop or by ?pilote=1', () => {
  assert.equal(parseProps({}, '').pilot, false);
  assert.equal(parseProps({ pilot: true }, '').pilot, true);
  assert.equal(parseProps({}, '?pilote=1').pilot, true);
  assert.equal(parseProps({}, '?pilote=0').pilot, false);
});

test('the window height follows the image, 2/3 for 800 × 1200', () => {
  const box = { x: 0, y: 0, w: 0.4 };
  const heightPx = heightOf(box, 800 / 1200, stage) * stage.height;
  assert.ok(Math.abs((box.w * stage.width) / heightPx - 2 / 3) < 1e-9);
});

test('a drag by the bar keeps the window whole on the stage', () => {
  const box = { x: 0.1, y: 0.1, w: 0.3 };
  const ratio = 2 / 3;
  assert.deepEqual(moved(box, 100, 80, ratio, stage), { x: 0.2, y: 0.2, w: 0.3 });
  const far = moved(box, 5000, 5000, ratio, stage);
  assert.equal(far.x, 0.7);
  assert.ok(Math.abs(far.y + heightOf(box, ratio, stage) - 1) < 1e-9);
  assert.deepEqual(moved(box, -5000, -5000, ratio, stage), { x: 0, y: 0, w: 0.3 });
});

test('the corner dragged to a square draws the largest window of the image’s ratio that fits it', () => {
  const ratio = 2 / 3;
  const box = { x: 0.1, y: 0.1, w: 0.2 };
  // 200 × 300 px, dragged to a 400 px square.
  const drag = dragged(box, 200, 100, ratio, stage);
  assert.ok(Math.abs(drag.w - 0.4) < 1e-9 && Math.abs(drag.h - 0.4) < 1e-9);
  const next = resized(box, drag, ratio, stage);
  assert.ok(Math.abs(next.w - 0.4 * ratio) < 1e-9);
  assert.ok(Math.abs(next.w / (next.w / ratio) - ratio) < 1e-9);
  assert.equal(next.x, box.x);
  assert.equal(next.y, box.y);
});

test('a window never shrinks to nothing nor leaves the stage by its width', () => {
  const ratio = 1;
  const box = { x: 0.5, y: 0, w: 0.3 };
  assert.equal(resized(box, dragged(box, -5000, -5000, ratio, stage), ratio, stage).w, 0.1);
  assert.ok(resized(box, dragged(box, 5000, 5000, ratio, stage), ratio, stage).w <= 0.5);
});

test('a window never grows past the bottom of the stage either', () => {
  const ratio = 0.5;
  const box = { x: 0.1, y: 0.6, w: 0.12 };
  const next = resized(box, dragged(box, 5000, 5000, ratio, stage), ratio, stage);
  assert.ok(next.w <= widest(box, ratio, stage) + 1e-9);
  assert.ok(box.y + heightOf(next, ratio, stage) <= 1 + 1e-9);
});

test('a row off the stage is brought back onto it', () => {
  assert.deepEqual(parseDiffusion({ window: { media: 'a', x: 5, y: -2, w: 40 } }).window, { media: 'a', x: 0, y: 0, w: 1 });
  assert.deepEqual(parseDiffusion({ window: { media: 'a', x: 0.9, y: 0.5, w: 0.3 } }).window, { media: 'a', x: 0.7, y: 0.5, w: 0.3 });
});

test('a command is named by identifier and aimed at the diffusion', () => {
  assert.deepEqual(commandBody('live.moveWindow', { campaign: 'c', x: 1, y: 2 }), {
    dataCapability: 'live.moveWindow@1',
    target: { aggregate: 'Diffusion' },
    payload: { campaign: 'c', x: 1, y: 2 },
    caller: { by: 'dm', from: 'PlayerStage', roles: [] },
  });
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['window-fields-together', 3] } }), { status: 'rejected', violations: ['window-fields-together'] });
});
