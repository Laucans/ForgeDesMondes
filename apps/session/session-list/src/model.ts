// What the list knows, with no DOM: the Capabilities' answers, the form the DM
// fills, the command that form becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Character, Form, Session } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);

/** The sessions as `session.listSessions` answers them, by number: a campaign with none is an empty list, not an error. */
export function parseSessions(data: unknown): Session[] {
  if (!Array.isArray(data)) return [];
  const sessions = data.filter(isRecord).map(
    (entry): Session => ({
      id: text(entry.id),
      order: typeof entry.order === 'number' ? entry.order : 0,
      title: text(entry.title),
      date: typeof entry.date === 'string' && entry.date !== '' ? entry.date : null,
      status: text(entry.status),
      label: typeof entry.label === 'string' && entry.label !== '' ? entry.label : null,
      present: texts(entry.present),
      summary: typeof entry.summary === 'string' && entry.summary !== '' ? entry.summary : null,
    }),
  );
  return sessions.sort((a, b) => a.order - b.order);
}

/** The characters as `campaign.listCharacters` answers them. */
export function parseCharacters(data: unknown): Character[] {
  if (!Array.isArray(data)) return [];
  return data.filter(isRecord).map((entry): Character => ({ id: text(entry.id), name: text(entry.name), kind: text(entry.kind) }));
}

/** `present-only-pj`: only a player's character can be ticked present. */
export const pcs = (characters: readonly Character[]): Character[] => characters.filter((character) => character.kind === 'pj');

/** A session already played: `closed`, which the mock-up's fixture writes `played`. */
export const isPlayed = (session: Session): boolean => session.status === 'closed' || session.status === 'played';

/** SessionLifecycle@1's label of a status, when the Capability did not bring it. */
export function statusLabel(session: Session): string {
  if (session.label !== null) return session.label;
  if (isPlayed(session)) return 'Jouée';
  return session.status === 'live' ? 'En direct' : 'En préparation';
}

/** The first name of a PC, as the list shows who was there; the id when the character is not known. */
export function presentName(characters: readonly Character[], id: string): string {
  const found = characters.find((character) => character.id === id);
  return (found?.name.split(' ')[0] ?? '') || id;
}

/** The title the form opens on: the session that would come next. */
export const defaultTitle = (sessions: readonly Session[]): string => `Séance ${sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1}`;

/** Why the form cannot be sent, or nothing. */
export function problem(form: Form): string {
  if (!form.title.trim()) return 'Donnez un titre à la séance.';
  if (!/^\d{4}-\d{2}-\d{2}$/.test(form.plannedOn)) return 'Choisissez la date de la séance.';
  return '';
}

/** The payload of session.createSession: only PCs of the campaign are sent as present. */
export function payload(campaign: string, form: Form, characters: readonly Character[]): { campaign: string; title: string; plannedOn: string; present: string[] } {
  const allowed = new Set(pcs(characters).map((character) => character.id));
  return { campaign, title: form.title.trim(), plannedOn: form.plannedOn, present: form.present.filter((id) => allowed.has(id)) };
}

/** `POST /commands` for one DataCapability, by identifier. */
export function commandBody(dataCapability: string, body: unknown, idempotencyKey: string): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, idempotencyKey, caller: { by: 'dm', from: 'SessionList', roles: [] } };
}

const VIOLATIONS: Record<string, string> = {
  'present-only-pj': 'seuls les PJ de la campagne peuvent être présents',
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
