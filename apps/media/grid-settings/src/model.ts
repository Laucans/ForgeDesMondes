// What the panel knows, with no DOM: the battlemap as the Capability answers
// it, the form the DM edits, the command that form becomes.

import { problems } from './hex.ts';
import { AGGREGATE, DATA_CAPABILITY_VERSION, SAVE_GRID_SETTINGS } from './ids.ts';
import type { Battlemap, Form, Grid, Orient, Shape } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const num = (value: unknown, otherwise: number): number => (typeof value === 'number' && Number.isFinite(value) ? value : otherwise);

/** The colours a grid may have: the design system's, by the name `color` carries. */
export const COLORS = [
  { value: 'text', label: 'Ivoire' },
  { value: 'accent', label: 'Ambre' },
  { value: 'primary', label: 'Orange' },
  { value: 'secondary', label: 'Violet' },
  { value: 'success', label: 'Vert' },
];

/** The CSS colour of a grid colour: a token, `text` for a name the design system does not know. */
export const cssColor = (color: string): string => `var(--color-${COLORS.some((c) => c.value === color) ? color : 'text'})`;

/** The shapes the map may draw: what the Capability's terrain is made of, nothing that runs. */
const TAGS = new Set(['rect', 'ellipse', 'circle', 'path', 'polygon', 'polyline', 'line']);
/** An attribute is a name of letters and dashes — never an event handler (`on…`). */
const ATTRIBUTE = /^(?!on)[a-z][a-z-]*$/;

function terrain(value: unknown): Shape[] {
  if (!Array.isArray(value)) return [];
  const shapes: Shape[] = [];
  for (const entry of value) {
    if (!isRecord(entry) || typeof entry.tag !== 'string' || !TAGS.has(entry.tag)) continue;
    const attrs: Record<string, string> = {};
    for (const [name, attribute] of Object.entries(entry)) {
      if (name !== 'tag' && ATTRIBUTE.test(name) && (typeof attribute === 'string' || typeof attribute === 'number')) attrs[name] = String(attribute);
    }
    shapes.push({ tag: entry.tag, attrs });
  }
  return shapes;
}

/** The Capability's answer: `null` when there is no battlemap (an illustration, an unknown id) or nothing readable. */
export function parseBattlemap(data: unknown): Battlemap | null {
  if (!isRecord(data) || typeof data.id !== 'string' || data.id === '' || !isRecord(data.grid)) return null;
  const grid = data.grid;
  const orient: Orient = grid.orient === 'flat' ? 'flat' : 'pointy';
  return {
    id: data.id,
    title: text(data.title),
    width: num(data.width, 800),
    height: num(data.height, 560),
    grid: { size: num(grid.size, 30), ox: num(grid.ox, 0), oy: num(grid.oy, 0), orient, color: text(grid.color) || 'text', opacity: num(grid.opacity, 0.4) },
    terrain: terrain(data.terrain),
  };
}

/** The form a battlemap opens on: its grid as last saved, the opacity shown as a percentage — a value out of range is shown as it is, and cannot be saved. */
export function initialForm(map: Battlemap): Form {
  return { ...map.grid, opacity: Math.round(map.grid.opacity * 100) };
}

/** The grid HexGrid@1 judges and the preview draws: the opacity back to a fraction of 1. */
export const gridOf = (form: Form): Grid => ({ ...form, opacity: form.opacity / 100 });

const WORDS: Record<string, string> = {
  size: 'La taille d’un hex doit être supérieure à 0.',
  ox: 'Le décalage horizontal doit être un nombre.',
  oy: 'Le décalage vertical doit être un nombre.',
  orient: 'L’orientation doit être « pointe en haut » ou « côté plat en haut ».',
  color: 'La couleur de la grille est obligatoire.',
  opacity: 'L’opacité doit être comprise entre 0 et 100 %.',
};

/** Why the form cannot be sent, or nothing: HexGrid@1's `problems` in words. */
export function problem(form: Form): string {
  return problems(gridOf(form)).map((field) => WORDS[field] ?? field).join(' ');
}

/** The payload of media.saveGridSettings: the six grid fields and nothing else — never the feet per hex, which is always 5. */
export function payload(form: Form): Grid {
  const { size, ox, oy, orient, color } = form;
  return { size, ox, oy, orient, color, opacity: form.opacity / 100 };
}

/** `POST /commands` for the save, by identifier: the battlemap is the Media it targets. */
export function commandBody(battlemap: string, body: Grid): unknown {
  return {
    dataCapability: `${SAVE_GRID_SETTINGS}@${DATA_CAPABILITY_VERSION}`,
    target: { aggregate: AGGREGATE, id: battlemap },
    payload: body,
    caller: { by: 'dm', from: 'GridSettings', roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'grid-values-range': 'la taille d’un hex doit être supérieure à 0 et l’opacité comprise entre 0 et 100 %',
  'grid-only-for-battlemap': 'seule une battlemap a une grille',
  'hex-is-five-feet': 'un hex vaut toujours 5 pieds',
};

/** A rejected command's reasons, in words. */
export const reasons = (violations: readonly string[]): string => violations.map((v) => VIOLATIONS[v] ?? v).join(', ');

/** The statuses after which a command no longer moves. */
const FINAL = new Set(['applied', 'rejected', 'cancelled', 'expired']);

export interface Settled {
  status: string;
  violations: string[];
}

/** `{ result: { status, violations } }`, as the server answers a command and its lookup. */
export function settled(answer: unknown): Settled {
  const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
  const violations = Array.isArray(result.violations) ? result.violations.filter((v): v is string => typeof v === 'string') : [];
  return { status: text(result.status), violations };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
