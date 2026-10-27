// What the panel knows, with no DOM: the Capabilities' answers reduced to the
// elements it lists, the groups they fall in, the search that finds them
// (names and aliases, normalised per AliasMatch@1) and the commands it sends.

import { AGGREGATE, DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Group, Item, Library, Medium, Scene, World } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : []);
const records = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? value.filter(isRecord) : []);

// ---------- AliasMatch@1 ----------

/** The accents of a decomposed letter: the combining marks U+0300 to U+036F. */
const ACCENTS = /[\u0300-\u036f]/g;

/**
 * AliasMatch@1's normalised term: lowercase, the accents removed and the letter recomposed, the white
 * space around it trimmed. The spaces inside and the letters that do not decompose (œ, æ, ß) stay.
 */
export function normalize(term: string): string {
  return term.toLowerCase().normalize('NFD').replace(ACCENTS, '').normalize('NFC').trim();
}

/** AliasMatch@1: two terms match when their normalised terms are equal and not empty. */
export function same(term: string, other: string): boolean {
  const normalized = normalize(term);
  return normalized !== '' && normalized === normalize(other);
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

const WORDS = /[^\p{L}\p{N}]+/u;
const wordsOf = (term: string): string[] => term.split(WORDS).filter((w) => w.length > 2);

/**
 * How well an element answers a search: 2 when a normalised name or alias contains the normalised term,
 * 1 when each of its words is within one letter (two for a long word) of a word of the names, 0 otherwise.
 * A blank term matches nothing.
 */
export function score(item: Pick<Item, 'name' | 'aliases'>, query: string): 0 | 1 | 2 {
  const term = normalize(query);
  if (term === '') return 0;
  const names = [item.name, ...item.aliases].map(normalize);
  if (names.some((n) => n.includes(term))) return 2;
  const known = wordsOf(names.join(' '));
  const asked = wordsOf(term);
  return asked.length > 0 && asked.every((w) => known.some((k) => distance(w, k) <= (w.length > 5 ? 2 : 1))) ? 1 : 0;
}

/** The elements the search finds, the best match first; a blank search finds none. */
export function search(items: readonly Item[], query: string): Item[] {
  return items
    .map((item) => ({ item, s: score(item, query) }))
    .filter((found) => found.s > 0)
    .sort((a, b) => b.s - a.s)
    .map((found) => found.item);
}

// ---------- the Capabilities' answers ----------

function entity(row: Record<string, unknown>, type: Item['type']): Item | null {
  const id = text(row.id);
  if (id === '') return null;
  const kind = row.kind === 'pj' || row.kind === 'pnj' ? row.kind : undefined;
  return { type, id, name: text(row.name), aliases: texts(row.aliases), ...(kind ? { kind } : {}) };
}

function entities(data: unknown, type: Item['type']): Item[] {
  return records(data).flatMap((row) => {
    const item = entity(row, type);
    return item === null ? [] : [item];
  });
}

/** campaign.listCharacters' answer: PCs and NPCs. */
export const parseCharacters = (data: unknown): Item[] => entities(data, 'character');
/** campaign.listPlaces' answer. */
export const parsePlaces = (data: unknown): Item[] => entities(data, 'place');
/** campaign.listEvents' answer. */
export const parseEvents = (data: unknown): Item[] => entities(data, 'event');

/** session.listScenes' answer, in the order it gives. */
export function parseScenes(data: unknown): Scene[] {
  return records(data).flatMap((row) => {
    const id = text(row.id);
    if (id === '') return [];
    const battlemap = text(row.battlemap);
    return [{ id, title: text(row.title), places: texts(row.places), characters: texts(row.characters), events: texts(row.events), media: texts(row.media), battlemap: battlemap === '' ? null : battlemap }];
  });
}

function media(data: unknown): Medium[] {
  return records(data).flatMap((row) => (text(row.id) === '' ? [] : [{ id: text(row.id), title: text(row.title) }]));
}

/** media.listMedia's answer: the battlemaps and the illustrations of the library. */
export function parseLibrary(data: unknown): Library {
  const answer = isRecord(data) ? data : {};
  return { battlemaps: media(answer.battlemaps), illustrations: media(answer.illustrations) };
}

// ---------- the elements ----------

/** A medium of the library as an element of the panel. */
const medium = (m: Medium, type: 'battlemap' | 'illustration'): Item => ({ type, id: m.id, name: m.title, aliases: [] });

/**
 * The battlemap of the library a scene's instance is of: the one it links, when it links exactly one. The
 * instance does not name its map here, so with two the choice would be a guess: none is chosen.
 */
function mapOf(scene: Scene, library: Library): string | undefined {
  const maps = scene.media.filter((id) => library.battlemaps.some((m) => m.id === id));
  return maps.length === 1 ? maps[0] : undefined;
}

/** The battlemaps of the library the session holds an instance of: the scene that links one gives its instance. */
function instances(world: World): Map<string, string> {
  const held = new Map<string, string>();
  for (const scene of world.scenes) {
    const map = mapOf(scene, world.library);
    if (map !== undefined && scene.battlemap !== null && !held.has(map)) held.set(map, scene.battlemap);
  }
  return held;
}

/** Every element of the panel, what the search looks through: the characters, places, events, then the battlemaps and illustrations. */
export function buildIndex(world: World): Item[] {
  const held = instances(world);
  return [
    ...world.characters,
    ...world.places,
    ...world.events,
    ...world.library.battlemaps.map((m) => {
      const instance = held.get(m.id);
      return instance === undefined ? medium(m, 'battlemap') : { ...medium(m, 'battlemap'), instance };
    }),
    ...world.library.illustrations.map((m) => medium(m, 'illustration')),
  ];
}

/** The elements of the index a list of ids names, in the list's order; an id the index lacks is left out. */
const named = (index: readonly Item[], type: Item['type'], ids: readonly string[]): Item[] =>
  ids.flatMap((id) => {
    const found = index.find((item) => item.type === type && item.id === id);
    return found === undefined ? [] : [found];
  });

/** A scene's places, NPCs, events and media, each run labelled; an empty run is not listed. */
export function sceneGroups(scene: Scene, world: World, index: readonly Item[]): Group[] {
  const map = mapOf(scene, world.library);
  const library = scene.media.flatMap((id) => index.filter((item) => item.id === id && (item.type === 'battlemap' || item.type === 'illustration')));
  // The scene's own instance, whatever the session holds of the same map elsewhere; one with no map linked is still the scene's.
  const own = library.map((item) => (item.type === 'battlemap' ? (item.id === map && scene.battlemap !== null ? { ...item, instance: scene.battlemap } : item) : item));
  const loose: Item[] = map === undefined && scene.battlemap !== null ? [{ type: 'battlemap', id: scene.battlemap, name: 'Battlemap de la scène', aliases: [], instance: scene.battlemap }] : [];
  const groups: Group[] = [
    { key: 'lieux', label: 'Lieux', items: named(index, 'place', scene.places) },
    { key: 'pnj', label: 'PNJ', items: named(index, 'character', scene.characters).filter((c) => c.kind === 'pnj') },
    { key: 'evenements', label: 'Événements', items: named(index, 'event', scene.events) },
    { key: 'medias', label: 'Médias', items: [...loose, ...own] },
  ];
  return groups.filter((g) => g.items.length > 0);
}

/** The whole campaign by type, every group listed even when empty. */
export function campaignGroups(index: readonly Item[]): Group[] {
  const of = (type: Item['type'], kind?: Item['kind']): Item[] => index.filter((item) => item.type === type && (kind === undefined || item.kind === kind));
  return [
    { key: 'pj', label: 'PJ', items: of('character', 'pj') },
    { key: 'pnj', label: 'PNJ', items: of('character', 'pnj') },
    { key: 'lieux', label: 'Lieux', items: of('place') },
    { key: 'evenements', label: 'Événements', items: of('event') },
    { key: 'medias', label: 'Médias', items: [...of('battlemap'), ...of('illustration')] },
  ];
}

// ---------- the commands ----------

/** `POST /commands` for one DataCapability, by identifier, aimed at the diffusion of the session when it is known. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, session: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    target: session === '' ? { aggregate: AGGREGATE } : { aggregate: AGGREGATE, id: session },
    payload,
    caller: { by: 'dm', from: 'ElementsPanel', roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'base-coherent': 'cette carte ou cette illustration n’est pas celle de la séance',
  'window-proportional': 'cette image ne peut pas être ouverte en fenêtre',
  'window-fields-together': 'la fenêtre n’a pas pu être ouverte',
  'single-diffusion-source': 'la diffusion a changé entre-temps',
  'unhandled-data-capability': 'commande inconnue du serveur',
};

/** A rejected command's reasons, in words. */
export const reasons = (violations: readonly string[]): string => violations.map((v) => VIOLATIONS[v] ?? v).join(', ');

/** The statuses after which a command no longer moves. */
const FINAL = new Set(['applied', 'rejected', 'cancelled', 'expired']);
export const isFinal = (status: string): boolean => FINAL.has(status);

export interface Settled {
  status: string;
  violations: string[];
}

/** `{ result: { status, violations } }`, as the server answers a command and its lookup. */
export function settled(answer: unknown): Settled {
  const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
  return { status: text(result.status), violations: texts(result.violations) };
}

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
