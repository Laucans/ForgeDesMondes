// What the Micro-UI knows, with no DOM: the library as the Capability answers
// it, the filters, the commands a click becomes and how a command is followed.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Library, Sound } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const num = (value: unknown, fallback: number): number => (typeof value === 'number' && Number.isFinite(value) ? value : fallback);
const clamp = (value: number, min: number, max: number): number => Math.min(max, Math.max(min, value));

function parseSound(value: Record<string, unknown>): Sound | null {
  const id = text(value.id);
  if (id === '') return null;
  return {
    id,
    name: text(value.name),
    tags: texts(value.tags),
    duration: typeof value.duration === 'number' && value.duration > 0 ? value.duration : null,
    volume: clamp(num(value.volume, 0), 0, 100),
    file: text(value.file),
    // A sound whose gain the Capability did not give plays silent: the gain is never worked out here.
    gain: clamp(num(value.gain, 0), 0, 1),
    playing: value.playing === true,
  };
}

/** The Capability's answer, however little of it there is: a campaign with no sound is an empty library, not an error. */
export function parseLibrary(data: unknown): Library {
  const answer = isRecord(data) ? data : {};
  const sounds = Array.isArray(answer.sounds) ? answer.sounds.filter(isRecord).map(parseSound).filter((sound): sound is Sound => sound !== null) : [];
  return { masterVolume: clamp(num(answer.masterVolume, 0), 0, 100), sounds };
}

/** A search matches the name or any one tag, whatever the case or the accents. */
export const fold = (value: string): string => value.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().trim();

/** A tag as the filter reads it (SoundTag@1): trimmed and lowercased, the accents kept. */
export const tagKey = (tag: string): string => tag.trim().toLowerCase();

/** Every tag of the library, once, in alphabetical order. */
export function allTags(sounds: readonly Sound[]): string[] {
  return Array.from(new Set(sounds.flatMap((sound) => sound.tags.map(tagKey)).filter((tag) => tag !== ''))).sort((a, b) => a.localeCompare(b, 'fr'));
}

/** The sounds that match the search and, when tags are chosen, carry at least one of them. */
export function visible(sounds: readonly Sound[], query: string, tags: ReadonlySet<string>): Sound[] {
  const needle = fold(query);
  return sounds.filter(
    (sound) => (needle === '' || [sound.name, ...sound.tags].some((field) => fold(field).includes(needle))) && (tags.size === 0 || sound.tags.some((tag) => tags.has(tagKey(tag)))),
  );
}

/** `POST /commands` for one DataCapability, by identifier. The DM's own, from this Micro-UI. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, targetId?: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    ...(targetId === undefined ? {} : { target: { id: targetId } }),
    caller: { by: 'dm', from: 'LiveSoundbox', roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'dm-side-only': 'seule la DM view joue un son',
  'sound-volume-range': 'le volume d’un son reste entre 0 et 100 %',
  'master-volume-range': 'le volume global reste entre 0 et 100 %',
  'stop-all': 'impossible d’arrêter les sons',
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

/** The seconds of a sound, as the mock-up writes them: « 2,4 s ». */
export const seconds = (duration: number | null): string => (duration === null ? '' : ` · ${String(duration).replace('.', ',')} s`);
