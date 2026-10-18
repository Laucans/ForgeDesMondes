// HexGrid@1 (concepts/HexGrid/v1), reimplemented here: what a hex grid is,
// when it is valid, where its hexes lie and what a distance of hexes reads in
// feet. Local on purpose — shared meaning is the Concept and its fixtures,
// never a shared library. No DOM.

import type { Grid, Hex } from './types.ts';

const SQRT3 = Math.sqrt(3);

/** One hex is 5 feet, whatever its size, offset or orientation, valid grid or not. */
export const FEET_PER_HEX = 5;

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);

/** The fields of a grid that do not hold, in the order size, ox, oy, orient, color, opacity; empty when the grid is valid. */
export function problems(grid: Readonly<Record<keyof Grid, unknown>>): Array<keyof Grid> {
  const out: Array<keyof Grid> = [];
  if (!finite(grid.size) || grid.size <= 0) out.push('size');
  if (!finite(grid.ox)) out.push('ox');
  if (!finite(grid.oy)) out.push('oy');
  if (grid.orient !== 'pointy' && grid.orient !== 'flat') out.push('orient');
  if (typeof grid.color !== 'string' || grid.color === '') out.push('color');
  if (!finite(grid.opacity) || grid.opacity < 0 || grid.opacity > 1) out.push('opacity');
  return out;
}

/** A distance of `hexes` hexes in feet; null unless `hexes` is a whole number from 0 up. */
export function feet(hexes: unknown): number | null {
  return typeof hexes === 'number' && Number.isInteger(hexes) && hexes >= 0 ? hexes * FEET_PER_HEX : null;
}

/** The centre of a hex, in image pixels. */
export function center(grid: Grid, hex: Hex): { x: number; y: number } {
  const s = grid.size;
  const x = grid.orient === 'pointy' ? s * SQRT3 * (hex.q + hex.r / 2) : s * 1.5 * hex.q;
  const y = grid.orient === 'pointy' ? s * 1.5 * hex.r : s * SQRT3 * (hex.r + hex.q / 2);
  return { x: x + grid.ox + s, y: y + grid.oy + s };
}

/** The fractional axial coordinates of a point of the image. */
function axial(grid: Grid, px: number, py: number): Hex {
  const s = grid.size;
  const x = px - (grid.ox + s);
  const y = py - (grid.oy + s);
  return grid.orient === 'pointy' ? { q: ((SQRT3 / 3) * x - y / 3) / s, r: ((2 / 3) * y) / s } : { q: ((2 / 3) * x) / s, r: ((-x / 3 + (SQRT3 / 3) * y)) / s };
}

/** The nearest hex to fractional axial coordinates. */
export function round(q: number, r: number): Hex {
  const y = -q - r;
  let rx = Math.round(q);
  let ry = Math.round(y);
  let rz = Math.round(r);
  const dx = Math.abs(rx - q);
  const dy = Math.abs(ry - y);
  const dz = Math.abs(rz - r);
  if (dx > dy && dx > dz) rx = -ry - rz;
  else if (dy > dz) ry = -rx - rz;
  else rz = -rx - ry;
  return { q: rx + 0, r: rz + 0 };
}

/** The hex under a point of the image. */
export function toHex(grid: Grid, px: number, py: number): Hex {
  const { q, r } = axial(grid, px, py);
  return round(q, r);
}

/** The number of hexes between two hexes. */
export function distance(a: Hex, b: Hex): number {
  const dq = a.q - b.q;
  const dr = a.r - b.r;
  return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
}

/** The hexes from `a` to `b`, both included. */
export function line(a: Hex, b: Hex): Hex[] {
  const n = distance(a, b);
  const out: Hex[] = [];
  for (let i = 0; i <= n; i += 1) {
    const t = n === 0 ? 0 : i / n;
    out.push(round(a.q + (b.q - a.q) * t + 1e-6, a.r + (b.r - a.r) * t + 2e-6));
  }
  return out;
}

/** The six corners of the hex centred on `c`. */
export function corners(grid: Grid, c: { x: number; y: number }): Array<[number, number]> {
  const offset = grid.orient === 'pointy' ? -30 : 0;
  return [0, 1, 2, 3, 4, 5].map((i) => {
    const angle = ((60 * i + offset) * Math.PI) / 180;
    return [c.x + grid.size * Math.cos(angle), c.y + grid.size * Math.sin(angle)];
  });
}

const pair = (p: [number, number]): string => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`;

/** The corners of a hex as an SVG `points` attribute. */
export const points = (grid: Grid, hex: Hex): string => corners(grid, center(grid, hex)).map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(' ');

/** More hexes than this over one image is a grid too fine to draw: it would freeze the screen, and no table plays on it. */
export const MAX_HEXES = 40000;

/** The `d` of the grid over an image of `width` x `height` pixels: every hex that touches it. Empty for a grid that cannot be drawn, or is too fine to. */
export function gridPath(grid: Grid, width: number, height: number): string {
  if (!finite(grid.size) || grid.size <= 0 || !finite(grid.ox) || !finite(grid.oy)) return '';
  const s = grid.size;
  // A hex covers about 2.6 s² of the image.
  if ((width * height) / (2.598 * s * s) > MAX_HEXES) return '';
  // Axial coordinates are linear in x and y: the image's four corners bound every hex it can hold.
  const edges = [axial(grid, -s, -s), axial(grid, width + s, -s), axial(grid, -s, height + s), axial(grid, width + s, height + s)];
  const qs = edges.map((e) => e.q);
  const rs = edges.map((e) => e.r);
  let d = '';
  for (let q = Math.floor(Math.min(...qs)) - 1; q <= Math.ceil(Math.max(...qs)) + 1; q += 1) {
    for (let r = Math.floor(Math.min(...rs)) - 1; r <= Math.ceil(Math.max(...rs)) + 1; r += 1) {
      const c = center(grid, { q, r });
      if (c.x < -s || c.x > width + s || c.y < -s || c.y > height + s) continue;
      d += `M${corners(grid, c).map(pair).join('L')}Z`;
    }
  }
  return d;
}
