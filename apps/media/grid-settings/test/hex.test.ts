import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { center, distance, feet, FEET_PER_HEX, gridPath, line, problems, toHex } from '../src/hex.ts';
import type { Grid } from '../src/types.ts';

interface Case {
  name: string;
  input: Record<string, unknown>;
  output: { valid: boolean; problems: string[]; feetPerHex: number; feet: number | null };
}

/** HexGrid@1's own fixtures: what the Concept says, this local reimplementation must say too. */
const { cases } = JSON.parse(readFileSync(new URL('../../../../concepts/HexGrid/v1/fixtures.json', import.meta.url), 'utf8')) as { cases: Case[] };

for (const c of cases) {
  test(`HexGrid@1: ${c.name}`, () => {
    const { hexes, ...grid } = c.input;
    const found = problems(grid as unknown as Grid);
    assert.deepEqual(found, c.output.problems);
    assert.equal(found.length === 0, c.output.valid);
    assert.equal(FEET_PER_HEX, c.output.feetPerHex);
    assert.equal(feet(hexes), c.output.feet);
  });
}

const grid: Grid = { size: 30, ox: 12, oy: 8, orient: 'pointy', color: 'text', opacity: 0.4 };

test('a hex under its own centre is that hex, in both orientations', () => {
  for (const orient of ['pointy', 'flat'] as const) {
    const g = { ...grid, orient };
    for (const hex of [{ q: 0, r: 0 }, { q: 3, r: -2 }, { q: -4, r: 5 }]) {
      const c = center(g, hex);
      assert.deepEqual(toHex(g, c.x, c.y), hex);
    }
  }
});

test('a distance is a whole number of hexes, 5 feet each, whatever the grid', () => {
  const a = { q: 1, r: 1 };
  const b = { q: 4, r: 3 };
  assert.equal(distance(a, b), 5);
  assert.equal(line(a, b).length, 6);
  assert.deepEqual([line(a, b)[0], line(a, b)[5]], [a, b]);
  assert.equal(feet(distance(a, b)), 25);
});

test('the grid path changes with each setting and covers the whole image', () => {
  const base = gridPath(grid, 960, 640);
  assert.ok(base.startsWith('M') && base.endsWith('Z'));
  for (const change of [{ size: 48 }, { ox: 30 }, { oy: 25 }, { orient: 'flat' as const }]) {
    assert.notEqual(gridPath({ ...grid, ...change }, 960, 640), base, JSON.stringify(change));
  }
  // The colour and the opacity are styles of the path, not part of its outline.
  assert.equal(gridPath({ ...grid, color: 'success', opacity: 0.2 }, 960, 640), base);
  // A larger image holds more hexes than a smaller one, whatever the offset.
  assert.ok(gridPath({ ...grid, ox: -37 }, 2000, 1500).length > gridPath({ ...grid, ox: -37 }, 500, 400).length);
});

test('a grid that cannot be drawn draws nothing', () => {
  assert.equal(gridPath({ ...grid, size: 0 }, 960, 640), '');
  assert.equal(gridPath({ ...grid, ox: Number.NaN }, 960, 640), '');
});
