// What the card knows, with no DOM: the three reads as the Capabilities
// answer them, how a medium stands against the session, the commands.

import { ADD_PLACE_TO_SESSION, DATA_CAPABILITY_VERSION, SET_MEDIA_PLACE, UPLOAD_MEDIA } from './ids.ts';
import type { Association, Draft, Kind, Medium, Place, Props } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const list = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? value.filter(isRecord) : []);

function medium(entry: Record<string, unknown>, kind: Kind): Medium {
  const grid = isRecord(entry.grid) ? { orient: text(entry.grid.orient), size: Number(entry.grid.size) || 0 } : null;
  return {
    id: text(entry.id),
    kind,
    title: text(entry.title),
    file: text(entry.file),
    size: text(entry.size),
    place: text(entry.place) || null,
    art: text(entry.art) || (kind === 'battlemap' ? 'forest' : 'ember'),
    glyph: text(entry.glyph) || (kind === 'battlemap' ? '⬡' : ''),
    grid: kind === 'battlemap' ? grid : null,
    ratio: typeof entry.ratio === 'number' ? entry.ratio : null,
  };
}

/** `media.listMedia`: the battlemaps, then the illustrations; a library with none is an empty one, not an error. */
export function parseMedia(data: unknown): Medium[] {
  const answer = isRecord(data) ? data : {};
  return [...list(answer.battlemaps).map((m) => medium(m, 'battlemap')), ...list(answer.illustrations).map((m) => medium(m, 'illustration'))].filter((m) => m.id !== '');
}

/** `campaign.listPlaces`: the campaign's places, by name. */
export function parsePlaces(data: unknown): Place[] {
  return list(data)
    .map((p) => ({ id: text(p.id), name: text(p.name) }))
    .filter((p) => p.id !== '');
}

/** `session.listScenes`: the ids of the places of the session's scenes — what attaches a medium to the session. */
export function parseSessionPlaces(data: unknown): Set<string> {
  return new Set(list(data).flatMap((scene) => texts(scene.places)));
}

/** The props, however the screen filled them. */
export function parseProps(raw: Record<string, unknown>): Props {
  const number = Number(raw.sessionNumber);
  return { session: text(raw.session), sessionNumber: Number.isInteger(number) && number > 0 ? number : null };
}

/**
 * session-media-by-place: a medium is attached to the session when its place is in one of the scenes.
 * A medium with no place is attached to nothing and is not an error; `places` is null while no session is known.
 */
export function association(m: Medium, places: ReadonlySet<string> | null): Association {
  if (m.place === null || places === null) return { kind: 'none' };
  return places.has(m.place) ? { kind: 'in', place: m.place } : { kind: 'out', place: m.place };
}

/** The battlemaps and the illustrations, each as the card lists them. */
export const byKind = (media: readonly Medium[], kind: Kind): Medium[] => media.filter((m) => m.kind === kind);

/** The upload form is ready when a file is chosen; the title falls back to the file's name. */
export function titleFor(draft: Draft, file: { name: string } | null): string {
  return draft.title.trim() || (file === null ? '' : stem(file.name)) || 'Sans titre';
}

/** `la-cave_du-sanglier.jpg` → `la cave du sanglier`. */
export const stem = (name: string): string => name.replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ');

/** `POST /commands` for one DataCapability, by identifier. */
function commandBody(dataCapability: string, payload: Record<string, unknown>, target: { aggregate: string; id: string } | undefined, idempotencyKey: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    ...(target === undefined ? {} : { target }),
    idempotencyKey,
    caller: { by: 'dm', from: 'MediaLibrary', roles: [] },
  };
}

/** media.setMediaPlace: the medium's place, or none (`place: null`). */
export const setPlaceCommand = (media: string, place: string | null, key: string): unknown => commandBody(SET_MEDIA_PLACE, { media, place }, { aggregate: 'Media', id: media }, key);

/** session.addPlaceToSession: the place joins the session; its media follow, derived at read time. */
export const addPlaceCommand = (session: string, place: string, key: string): unknown => commandBody(ADD_PLACE_TO_SESSION, { session, place }, { aggregate: 'Session', id: session }, key);

export interface Upload {
  kind: Kind;
  title: string;
  /** The file the DM chose: the name the browser gives, no copy is made. */
  filePath: string;
  place: string | null;
  size: { width: number; height: number } | null;
}

/** media.uploadMedia: a medium with no place is accepted, so the place is left out when there is none. */
export function uploadCommand(upload: Upload, key: string): unknown {
  const payload: Record<string, unknown> = { kind: upload.kind, title: upload.title, filePath: upload.filePath };
  if (upload.size !== null) Object.assign(payload, { widthPx: upload.size.width, heightPx: upload.size.height });
  if (upload.place !== null) payload.place = upload.place;
  return commandBody(UPLOAD_MEDIA, payload, undefined, key);
}

const VIOLATIONS: Record<string, string> = {
  'media-place-is-lieu': 'ce lieu n’est pas un lieu de la campagne',
  'scene-links-same-campaign': 'ce lieu n’est pas de la campagne de la séance',
  'grid-only-for-battlemap': 'seule une battlemap a une grille',
  'unhandled-data-capability': 'cette action n’est pas disponible',
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
  return { status: text(result.status), violations: texts(result.violations) };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
