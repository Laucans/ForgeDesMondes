// What the board knows, with no DOM: the answers of the Capabilities, the
// order of the scenes, the commands a click becomes.

import { APPROVER, DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Board, Character, Entity, Instance, Media, Medias, Scene, Session } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const records = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? value.filter(isRecord) : []);

/** The session, however little of it there is: an unknown answer is an empty session, not an error. */
export function parseSession(data: unknown): Session {
  const answer = isRecord(data) ? data : {};
  return {
    id: text(answer.id),
    title: text(answer.title),
    order: typeof answer.order === 'number' ? answer.order : null,
    campaign: typeof answer.campaign === 'string' ? answer.campaign : null,
    present: texts(answer.present),
  };
}

/** The scenes, in the order of their rank. */
export function parseScenes(data: unknown): Scene[] {
  return records(data)
    .map((scene, index) => ({
      scene: {
        id: text(scene.id),
        title: text(scene.title),
        places: texts(scene.places),
        characters: texts(scene.characters),
        events: texts(scene.events),
        media: texts(scene.media),
        toCreate: texts(scene.toCreate),
        battlemap: typeof scene.battlemap === 'string' && scene.battlemap !== '' ? scene.battlemap : null,
        notes: text(scene.notes),
      } satisfies Scene,
      rank: typeof scene.order === 'number' ? scene.order : index + 1,
    }))
    .filter(({ scene }) => scene.id !== '')
    .sort((a, b) => a.rank - b.rank)
    .map(({ scene }) => scene);
}

export function parseCharacters(data: unknown): Character[] {
  return records(data)
    .filter((character) => text(character.id) !== '')
    .map((character): Character => ({ id: text(character.id), name: text(character.name) || text(character.id), kind: character.kind === 'pj' ? 'pj' : 'pnj' }));
}

/** The places or the events of the campaign: an id, and the name a scene shows for it (the id when the answer gives none). */
export function parseEntities(data: unknown): Entity[] {
  return records(data)
    .filter((entity) => text(entity.id) !== '')
    .map((entity): Entity => ({ id: text(entity.id), name: text(entity.name) || text(entity.id) }));
}

const medias = (value: unknown): Media[] =>
  records(value)
    .filter((media) => text(media.id) !== '')
    .map((media) => ({ id: text(media.id), title: text(media.title) || text(media.id) }));

export function parseMedias(data: unknown): Medias {
  const answer = isRecord(data) ? data : {};
  return { battlemaps: medias(answer.battlemaps), illustrations: medias(answer.illustrations) };
}

/** An instance as the DM view answers it; null when it is not one. */
export function parseInstance(data: unknown): Instance | null {
  if (!isRecord(data) || text(data.id) === '') return null;
  const tokens = isRecord(data.state) ? records(data.state.tokens) : [];
  return { id: text(data.id), battlemap: text(data.battlemap), tokens: tokens.length, hidden: tokens.filter((token) => token.visible === false).length };
}

/** The campaign the Capabilities are read for: the one session.getSession gives, else the `campaign` prop; null when neither names one. */
export const campaignAsked = (fromSession: string | null, prop: unknown): string | null => {
  if (fromSession !== null && fromSession !== '') return fromSession;
  return typeof prop === 'string' && prop !== '' ? prop : null;
};

/** The session the board is for: its `session` prop, else the screen's context value; '' when neither names one. */
export const sessionAsked = (prop: unknown, context: unknown): string => {
  if (typeof prop === 'string' && prop !== '') return prop;
  return typeof context === 'string' ? context : '';
};

// present-only-pj: a PC of the session's campaign is the only one that can be ticked.
export const pcs = (board: Pick<Board, 'characters'>): Character[] => board.characters.filter((character) => character.kind === 'pj');

/** The PCs present, in the order the session lists them; an id the campaign does not know is not shown. */
export const presentPcs = (board: Pick<Board, 'characters' | 'session'>): Character[] =>
  board.session.present.flatMap((id) => pcs(board).filter((character) => character.id === id));

/** The PCs of the campaign still to tick. */
export const absentPcs = (board: Pick<Board, 'characters' | 'session'>): Character[] => pcs(board).filter((character) => !board.session.present.includes(character.id));

/** scene-order-contiguous: every scene id once, in the order they play (the server ranks them 1 to n); the scene `id` moved by `delta` places, null when it cannot move that way. */
export function moved(scenes: readonly Scene[], id: string, delta: number): string[] | null {
  const from = scenes.findIndex((scene) => scene.id === id);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= scenes.length) return null;
  const order = scenes.map((scene) => scene.id);
  order.splice(to, 0, ...order.splice(from, 1));
  return order;
}

/** `POST /commands` for one DataCapability, by identifier. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, target: { aggregate: string; id: string }, idempotencyKey?: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    target,
    ...(idempotencyKey === undefined ? {} : { idempotencyKey }),
    caller: { by: 'dm', from: 'SceneBoard', roles: [APPROVER] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'present-only-pj': 'seul un PJ de la campagne peut être présent',
  'scene-order-contiguous': 'les rangs des scènes doivent aller de 1 à n',
  'one-instance-per-scene': 'cette scène a déjà une instance de battlemap',
};

/** A rejected command's reasons, in words. */
export const reasons = (violations: readonly string[]): string => violations.map((violation) => VIOLATIONS[violation] ?? violation).join(', ');

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
