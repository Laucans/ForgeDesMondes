import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { center, corners, distance, feet, gridPath, label, line, onMap, toHex } from '../src/hexgrid.ts';
import type { Grid } from '../src/types.ts';

// The grid of the mock-up's battlemap (HexGrid@1's first case): 6 hexes read 30 feet.
const POINTY: Grid = { size: 30, ox: 12, oy: 8, orient: 'pointy', color: 'text', opacity: 0.4 };
const FLAT: Grid = { ...POINTY, size: 64, ox: 0, oy: 0, orient: 'flat' };

describe('HexGrid@1 distance in feet', () => {
  test('a hex is always 5 feet', () => {
    assert.equal(feet(6), 30);
    assert.equal(feet(0), 0);
    assert.equal(feet(1), 5);
    assert.equal(feet(2.0), 10);
  });

  test('a distance that is not a whole number from 0 up reads no feet', () => {
    for (const bad of [-2, 2.5, '6', null, undefined, Number.NaN]) assert.equal(feet(bad), null, String(bad));
  });

  test('the counter reads « n hex · n×5 pi »', () => {
    assert.equal(label(3), '3 hex · 15 pi');
    assert.equal(label(0), '0 hex · 0 pi');
    assert.equal(label(6), '6 hex · 30 pi');
  });
});

describe('hex geometry', () => {
  test('the distance between two hexes counts hex steps', () => {
    assert.equal(distance({ q: 0, r: 0 }, { q: 0, r: 0 }), 0);
    assert.equal(distance({ q: 0, r: 6 }, { q: 3, r: 6 }), 3);
    assert.equal(distance({ q: 0, r: 0 }, { q: 2, r: 2 }), 4);
    assert.equal(distance({ q: 0, r: 0 }, { q: -3, r: 3 }), 3);
    assert.equal(distance({ q: 5, r: -2 }, { q: 1, r: 1 }), distance({ q: 1, r: 1 }, { q: 5, r: -2 }));
  });

  test('the centre of a hex falls back in that hex, whatever the orientation', () => {
    for (const grid of [POINTY, FLAT]) {
      for (let q = -4; q <= 6; q += 1) {
        for (let r = -4; r <= 6; r += 1) assert.deepEqual(toHex(grid, center(grid, { q, r })), { q, r }, `${grid.orient} ${q},${r}`);
      }
    }
  });

  test('a point near the border still belongs to one hex of the neighbourhood', () => {
    const c = center(POINTY, { q: 2, r: 2 });
    const hex = toHex(POINTY, { x: c.x + POINTY.size * 0.4, y: c.y + POINTY.size * 0.2 });
    assert.ok(distance(hex, { q: 2, r: 2 }) <= 1);
  });

  test('a line from a hex to another has every step between them', () => {
    const steps = line({ q: 0, r: 6 }, { q: 3, r: 6 });
    assert.equal(steps.length, 4);
    assert.deepEqual(steps[0], { q: 0, r: 6 });
    assert.deepEqual(steps[3], { q: 3, r: 6 });
    for (let i = 1; i < steps.length; i += 1) assert.equal(distance(steps[i - 1] as { q: number; r: number }, steps[i] as { q: number; r: number }), 1);
    assert.deepEqual(line({ q: 1, r: 1 }, { q: 1, r: 1 }), [{ q: 1, r: 1 }]);
  });

  test('the grid is drawn over the image, and only a grid of hexes above 0 is', () => {
    assert.match(gridPath(POINTY, 960, 640), /^M[\d. -]+(L[\d. -]+)+Z/);
    assert.equal(gridPath({ ...POINTY, size: 0 }, 960, 640), '');
  });

  test('a small hex on a large image is drawn to the last corner of the image', () => {
    const small: Grid = { ...POINTY, size: 8, ox: 0, oy: 0 };
    const hex = toHex(small, { x: 1590, y: 1190 });
    const [corner] = corners(small, hex);
    assert.ok(gridPath(small, 1600, 1200).includes(`M${(corner as { x: number }).x.toFixed(1)} `), 'the hex at the far corner is in the path');
    const flat: Grid = { ...small, orient: 'flat' };
    const far = toHex(flat, { x: 1590, y: 1190 });
    assert.ok(gridPath(flat, 1600, 1200).includes(`${(corners(flat, far)[0] as { x: number }).x.toFixed(1)} `));
  });

  test('a hex is on the map while its centre is on the image', () => {
    assert.equal(onMap(POINTY, { q: 3, r: 6 }, 960, 640), true);
    assert.equal(onMap(POINTY, { q: 60, r: 6 }, 960, 640), false);
    assert.equal(onMap(POINTY, { q: 0, r: -3 }, 960, 640), false);
    assert.equal(onMap(POINTY, { q: 0, r: 40 }, 960, 640), false);
  });
});
