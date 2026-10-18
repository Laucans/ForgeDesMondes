// HexGrid@1 (concepts/HexGrid/v1), the part a map on screen needs: where a
// hex is, which hex a point falls in, how far two hexes are, what that
// distance reads in feet. Axial coordinates (q, r); one hex is always 5 feet,
// whatever the grid's size, offset and orientation.

import type { Grid, Hex } from './types.ts';

export const FEET_PER_HEX = 5;
const SQRT3 = Math.sqrt(3);

export interface Point {
  x: number;
  y: number;
}

/** `hexes` read in feet: null unless it is a whole number from 0 up. */
export const feet = (hexes: unknown): number | null => (typeof hexes === 'number' && Number.isInteger(hexes) && hexes >= 0 ? hexes * FEET_PER_HEX : null);

/** The counter shown while a token is carried: « 3 hex · 15 pi ». */
export const label = (hexes: number): string => `${hexes} hex · ${feet(hexes) ?? 0} pi`;

/** The centre of a hex, in image pixels. */
export function center(grid: Grid, hex: Hex): Point {
  const s = grid.size;
  const [x, y] = grid.orient === 'pointy' ? [s * SQRT3 * (hex.q + hex.r / 2), s * 1.5 * hex.r] : [s * 1.5 * hex.q, s * SQRT3 * (hex.r + hex.q / 2)];
  return { x: x + grid.ox + s, y: y + grid.oy + s };
}

/** Cube rounding: the hex a fractional (q, r) falls in. */
function round(q: number, r: number): Hex {
  const x = q;
  const z = r;
  const y = -x - z;
  let rx = Math.round(x);
  const ry0 = Math.round(y);
  let rz = Math.round(z);
  const dx = Math.abs(rx - x);
  const dy = Math.abs(ry0 - y);
  const dz = Math.abs(rz - z);
  if (dx > dy && dx > dz) rx = -ry0 - rz;
  else if (dy <= dz) rz = -rx - ry0;
  // `+ 0` turns a negative zero into 0.
  return { q: rx + 0, r: rz + 0 };
}

/** The hex an image point falls in. */
export function toHex(grid: Grid, point: Point): Hex {
  const s = grid.size;
  const x = point.x - grid.ox - s;
  const y = point.y - grid.oy - s;
  return grid.orient === 'pointy' ? round((SQRT3 / 3 * x - y / 3) / s, (2 / 3 * y) / s) : round((2 / 3 * x) / s, (-x / 3 + SQRT3 / 3 * y) / s);
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

/** The six corners of a hex. */
export function corners(grid: Grid, hex: Hex): Point[] {
  const c = center(grid, hex);
  const offset = grid.orient === 'pointy' ? -30 : 0;
  return Array.from({ length: 6 }, (_, i) => {
    const angle = ((60 * i + offset) * Math.PI) / 180;
    return { x: c.x + grid.size * Math.cos(angle), y: c.y + grid.size * Math.sin(angle) };
  });
}

const points = (list: Point[]): string => list.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' ');

/** A hex as the `points` of an SVG polygon. */
export const polygon = (grid: Grid, hex: Hex): string => points(corners(grid, hex));

/** A polyline through the centres of `hexes`. */
export const trail = (grid: Grid, hexes: Hex[]): string => points(hexes.map((h) => center(grid, h)));

/** Whether a hex stands on the image: its centre is inside it. */
export function onMap(grid: Grid, hex: Hex, width: number, height: number): boolean {
  const c = center(grid, hex);
  return c.x >= 0 && c.x <= width && c.y >= 0 && c.y <= height;
}

/** The grid over a `width` × `height` image, as one SVG path. */
export function gridPath(grid: Grid, width: number, height: number): string {
  if (!(grid.size > 0)) return '';
  const s = grid.size;
  // No hex of the image is further than this many hexes from the origin, whatever the orientation.
  const n = Math.ceil(Math.max(width, height) / s) + 2;
  let d = '';
  for (let q = -n; q <= n; q += 1) {
    for (let r = -n; r <= n; r += 1) {
      const c = center(grid, { q, r });
      if (c.x < -s || c.x > width + s || c.y < -s || c.y > height + s) continue;
      d += `M${corners(grid, { q, r }).map((p) => `${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join('L')}Z`;
    }
  }
  return d;
}
