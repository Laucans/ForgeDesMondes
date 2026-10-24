// What the panel knows without a page: how the six answers are read, how the
// rail matches a search (AliasMatch@1 for the normal form), which line goes in
// which section and how the fold of the rail is remembered.

import type { Character, Group, Media, Named, Player, RailEntry, Sound } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const records = (value: unknown): Array<Record<string, unknown>> => (Array.isArray(value) ? value.filter(isRecord) : []);

/** AliasMatch@1: without accents, in lowercase, with no white space around it; a letter that does not decompose is kept. */
export function normalize(term: string): string {
  return term
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .normalize('NFC')
    .toLowerCase()
    .trim();
}

export function parsePlayers(data: unknown): Player[] {
  return records(data).map((row) => ({ id: text(row.id), name: text(row.name) }));
}

export function parseCharacters(data: unknown): Character[] {
  return records(data).map((row) => {
    const ddb = isRecord(row.ddb) ? row.ddb : null;
    return {
      id: text(row.id),
      name: text(row.name),
      aliases: texts(row.aliases),
      kind: row.kind === 'pj' ? 'pj' : 'pnj',
      player: typeof row.player === 'string' ? row.player : null,
      initials: text(row.initials),
      type: text(row.type),
      art: text(row.art),
      ddbSyncedAt: ddb && typeof ddb.syncedAt === 'string' ? ddb.syncedAt : null,
      hasDdb: ddb !== null,
    };
  });
}

/** Places and events share one shape. */
export function parseElements(data: unknown): Named[] {
  return records(data).map((row) => ({ id: text(row.id), name: text(row.name), aliases: texts(row.aliases), art: text(row.art) }));
}

export function parseMedia(data: unknown): Media[] {
  const library = isRecord(data) ? data : {};
  const size = (row: Record<string, unknown>): string => {
    if (typeof row.size === 'string') return row.size;
    return typeof row.width === 'number' && typeof row.height === 'number' ? `${row.width}×${row.height}` : '';
  };
  return [
    ...records(library.battlemaps).map((row): Media => ({ id: text(row.id), title: text(row.title), kind: 'battlemap', size: size(row), art: 'forest', glyph: '⬡' })),
    ...records(library.illustrations).map((row): Media => {
      const title = text(row.title);
      return { id: text(row.id), title, kind: 'illustration', size: size(row), art: text(row.art) || 'night', glyph: text(row.glyph) || title.charAt(0) };
    }),
  ];
}

export function parseSounds(data: unknown): Sound[] {
  return records(isRecord(data) ? data.sounds : null).map((row) => ({ id: text(row.id), name: text(row.name), tags: texts(row.tags) }));
}

/** The level a character's text states (« rôdeuse niveau 5 »), else a dash. */
export function level(type: string): string {
  return /niveau (\d+)/.exec(type)?.[1] ?? '—';
}

/** « Synchronisé le … » when the sheet was imported from D&D Beyond, else a dash. */
export function synced(character: Character, format: (at: string) => string): string {
  return character.ddbSyncedAt === null ? '—' : `Synchronisé le ${format(character.ddbSyncedAt)}`;
}

/** The leading article of a place does not make its initial: « La Tour d’Ébène » is a T. */
export function placeGlyph(name: string): string {
  return name.replace(/^(La |Le |L’)/, '').charAt(0);
}

export const GROUPS: ReadonlyArray<readonly [Group, string]> = [
  ['pj', 'PJ'],
  ['pnj', 'PNJ'],
  ['place', 'Lieux'],
  ['event', 'Événements'],
];

/** The rail's lines: PJ, PNJ, places and events; media and sounds stay on the campaign page. */
export function railIndex(characters: Character[], places: Named[], events: Named[]): RailEntry[] {
  return [
    ...characters.map((c): RailEntry => ({ id: c.id, group: c.kind, name: c.name, aliases: c.aliases, sub: c.aliases.join(', ') || c.type, art: c.art || 'night', glyph: c.initials })),
    ...places.map((p): RailEntry => ({ id: p.id, group: 'place', name: p.name, aliases: p.aliases, sub: p.aliases.join(', '), art: p.art || 'night', glyph: placeGlyph(p.name) })),
    ...events.map((v): RailEntry => ({ id: v.id, group: 'event', name: v.name, aliases: v.aliases, sub: v.aliases.join(', '), art: 'blood', glyph: '!' })),
  ];
}

function distance(a: string, b: string): number {
  const row = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i += 1) {
    let previous = row[0] ?? 0;
    row[0] = i;
    for (let j = 1; j <= b.length; j += 1) {
      const above = row[j] ?? 0;
      row[j] = Math.min(above + 1, (row[j - 1] ?? 0) + 1, previous + (a[i - 1] === b[j - 1] ? 0 : 1));
      previous = above;
    }
  }
  return row[b.length] ?? 0;
}

const words = (term: string): string[] => term.split(/[^\p{L}\p{N}]+/u).filter((word) => word.length > 2);

/**
 * How well a line answers the search: 2 when the name or an alias holds it
 * (AliasMatch@1's normal form), 1 when every word of it is within one letter
 * (two for a long word) of a word of the name or an alias, else 0. A blank
 * search matches nothing.
 */
export function score(entry: Pick<RailEntry, 'name' | 'aliases'>, query: string): number {
  const wanted = normalize(query);
  if (wanted === '') return 0;
  const names = [entry.name, ...entry.aliases].map(normalize);
  if (names.some((name) => name.includes(wanted))) return 2;
  const known = words(names.join(' '));
  const asked = words(wanted);
  return asked.length > 0 && asked.every((word) => known.some((other) => distance(word, other) <= (word.length > 5 ? 2 : 1))) ? 1 : 0;
}

/** The lines the search keeps, best first, whatever the section; no search keeps them all in order. */
export function search(index: RailEntry[], query: string): RailEntry[] {
  if (query.trim() === '') return index;
  return index
    .map((entry) => ({ entry, score: score(entry, query) }))
    .filter((hit) => hit.score > 0)
    .sort((a, b) => b.score - a.score)
    .map((hit) => hit.entry);
}

export function countText(query: string, found: number): string {
  const q = query.trim();
  if (q === '') return '';
  return found > 0 ? `${found} résultat${found > 1 ? 's' : ''} pour « ${q} »` : `Rien pour « ${q} »`;
}

/** The key under which the browser remembers the rail's fold. */
export const FOLD_KEY = 'CampaignElements:rail';

/** The minimal part of `Storage` the fold needs; a browser that refuses it just forgets. */
export interface Memory {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function readFolded(memory: Memory | null): boolean {
  try {
    const kept = memory?.getItem(FOLD_KEY);
    const parsed: unknown = kept ? JSON.parse(kept) : null;
    return isRecord(parsed) && parsed.collapsed === true;
  } catch {
    return false;
  }
}

export function writeFolded(memory: Memory | null, folded: boolean): void {
  try {
    memory?.setItem(FOLD_KEY, JSON.stringify({ collapsed: folded }));
  } catch {
    // Storage is unavailable or full: the fold lasts until the page is left.
  }
}
