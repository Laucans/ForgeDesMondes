// What the card knows, with no DOM: the sounds as the Capability answers
// them, the search and the tag filter, the commands the forms become.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { AddForm, EditForm, Sound } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const num = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

/** The own volume, in percent, a sound starts with when it is added: the DM sets it afterwards, from the edit form. */
export const DEFAULT_VOLUME = 75;

/** The Capability's answer, however little of it there is: a campaign with no sound is an empty list, not an error. */
export function parseSounds(data: unknown): Sound[] {
  const list = isRecord(data) && Array.isArray(data.sounds) ? (data.sounds as unknown[]) : [];
  const sounds: Sound[] = [];
  for (const entry of list) {
    if (!isRecord(entry) || text(entry.id) === '') continue;
    sounds.push({
      id: text(entry.id),
      name: text(entry.name),
      tags: texts(entry.tags),
      volume: num(entry.volume) ?? 0,
      file: text(entry.file),
      duration: num(entry.duration),
      playing: entry.playing === true,
    });
  }
  return sounds;
}

/** The campaign of the running session, from `session.getLiveSession` (`{ session: { campaign } }`); empty when there is none. */
export function campaignOf(data: unknown): string {
  return isRecord(data) && isRecord(data.session) ? text(data.session.campaign) : '';
}

/** Case and accents out: « Épées » is found by « epees ». */
export const norm = (value: string): string => value.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim();

/**
 * What the list shows: the sounds whose name or tags hold the search, and
 * carry the tag when one is chosen. The two read nothing but these fields
 * (tags-normalized).
 */
export function filterSounds(sounds: readonly Sound[], search: string, tag: string | null): Sound[] {
  const wanted = norm(search);
  const chosen = tag === null ? null : norm(tag);
  return sounds.filter(
    (sound) =>
      (wanted === '' || norm(`${sound.name} ${sound.tags.join(' ')}`).includes(wanted)) && (chosen === null || sound.tags.some((t) => norm(t) === chosen)),
  );
}

/** The distinct tags of the library, as the first sound spells them. */
export function tagsOf(sounds: readonly Sound[]): string[] {
  const seen = new Map<string, string>();
  for (const sound of sounds) for (const tag of sound.tags) if (!seen.has(norm(tag))) seen.set(norm(tag), tag);
  return [...seen.values()];
}

/** « porte, bois , » → ['porte', 'bois']: what is typed in a Tags field. The server normalises them (SoundTag@1). */
export const splitTags = (value: string): string[] => value.split(',').map((tag) => tag.trim().replace(/^#/, '')).filter((tag) => tag !== '');

/** « volet-qui_claque.ogg » → « Volet qui claque »: the name a dropped file suggests. */
export function nameFromFile(fileName: string): string {
  const name = fileName.replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ').trim();
  return name === '' ? '' : name.charAt(0).toUpperCase() + name.slice(1);
}

/** The file types the library takes: the FileDrop's, by extension. */
export const FILE_TYPES = ['ogg', 'mp3', 'wav'] as const;

export const isSoundFile = (fileName: string): boolean => FILE_TYPES.some((type) => fileName.toLowerCase().endsWith(`.${type}`));

/** « 2,4 s », or a dash when the duration is not known. */
export const duration = (seconds: number | null): string => (seconds === null ? '—' : `${seconds.toFixed(1).replace('.', ',')} s`);

/** « 12 sons · 9 tags ». */
export function summary(sounds: readonly Sound[]): string {
  const count = sounds.length;
  const tags = tagsOf(sounds).length;
  return `${count} ${count > 1 ? 'sons' : 'son'} · ${tags} ${tags > 1 ? 'tags' : 'tag'}`;
}

/** Why the add form cannot be sent yet; '' when it can. */
export function addProblem(form: AddForm): string {
  if (form.file === '') return 'Déposez d’abord le fichier du son.';
  if (form.name.trim() === '') return 'Le nom du son est obligatoire.';
  return '';
}

/** Why the edit form cannot be sent yet; '' when it can. */
export function editProblem(form: EditForm): string {
  if (form.name.trim() === '') return 'Le nom du son est obligatoire.';
  if (!(form.volume >= 0 && form.volume <= 100)) return 'Le volume reste entre 0 et 100 %.';
  return '';
}

/** The payload of soundbox.addSound (data-capability.json): the volume in percent, the campaign when the screen names one. */
export function addPayload(form: AddForm, campaign: string): Record<string, unknown> {
  return { ...(campaign === '' ? {} : { campaign }), name: form.name.trim(), tags: splitTags(form.tags), volume: DEFAULT_VOLUME, file: form.file };
}

/** The payload of soundbox.updateSound: what the form edits, the sound being the command's target. */
export function updatePayload(form: EditForm): Record<string, unknown> {
  return { name: form.name.trim(), tags: splitTags(form.tags), volume: form.volume };
}

/** `POST /commands` for one DataCapability, by identifier; `targetId` is the sound it acts on. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, targetId?: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    ...(targetId === undefined ? {} : { targetId }),
    caller: { by: 'dm', from: 'SoundLibrary', roles: [] as string[] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'sound-volume-range': 'le volume reste entre 0 et 100 %',
  'tags-normalized': 'les tags sont en minuscules, sans doublon',
  'not-found': 'ce son n’existe plus',
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
