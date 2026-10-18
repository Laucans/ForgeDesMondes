// What the list knows, with no DOM: the sessions and characters as the
// Capabilities answer them, the form the DM edits, the command that form becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Character, Form, Session } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const optionalText = (value: unknown): string | null => (typeof value === 'string' ? value : null);
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);

/** The sessions as `session.listSessions` answers them, in the order of their position. */
export function parseSessions(data: unknown): Session[] {
  const sessions: Session[] = [];
  for (const entry of Array.isArray(data) ? data : []) {
    if (!isRecord(entry) || text(entry.id) === '') continue;
    sessions.push({
      id: text(entry.id),
      order: typeof entry.order === 'number' ? entry.order : 0,
      title: text(entry.title),
      date: optionalText(entry.date),
      status: text(entry.status),
      label: optionalText(entry.label),
      present: texts(entry.present),
      summary: optionalText(entry.summary),
    });
  }
  return sessions.sort((a, b) => a.order - b.order);
}

/** The characters as `campaign.listCharacters` answers them: only what the list shows of each. */
export function parseCharacters(data: unknown): Character[] {
  const characters: Character[] = [];
  for (const entry of Array.isArray(data) ? data : []) {
    if (isRecord(entry) && text(entry.id) !== '') characters.push({ id: text(entry.id), name: text(entry.name), kind: text(entry.kind) });
  }
  return characters;
}

/** Only the PCs can be ticked present (present-only-pj): an NPC is never offered. */
export const playerCharacters = (characters: readonly Character[]): Character[] => characters.filter((c) => c.kind === 'pj');

/** The first name of a present PC; an id the characters do not know is shown as it is. */
export const firstName = (characters: readonly Character[], id: string): string => (characters.find((c) => c.id === id)?.name ?? id).split(' ')[0] ?? id;

/** The session being prepared, or run, is the one the list draws attention to. */
export const isNext = (session: Session): boolean => session.status === 'prep' || session.status === 'live';

/** The status in words: the label SessionLifecycle@1 gives it, else the one for the status the mock-up knows. */
export function statusLabel(session: Session): string {
  if (session.label) return session.label;
  return session.status === 'prep' ? 'En préparation' : session.status === 'live' ? 'En direct' : 'Jouée';
}

/** The tone of the kit's Badge for the status. */
export const statusTone = (session: Session): 'accent' | 'primary' | 'success' => (session.status === 'prep' ? 'accent' : session.status === 'live' ? 'primary' : 'success');

/** « sam. 5 septembre », or nothing for a session with no date. */
export function dateLabel(date: string | null): string {
  if (date === null || date === '') return '';
  // Only the day counts: a date that carries a time is read at noon, away from the edges of the day.
  const parsed = new Date(`${date.slice(0, 10)}T12:00:00`);
  return Number.isNaN(parsed.getTime()) ? date : parsed.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric', month: 'long' });
}

/** The number the next session takes: one after the last. */
export const nextOrder = (sessions: readonly Session[]): number => sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1;

/** The form the DM opens: a title proposed, no date, every PC ticked. */
export function initialForm(sessions: readonly Session[], characters: readonly Character[]): Form {
  return { title: `Séance ${nextOrder(sessions)}`, date: '', present: playerCharacters(characters).map((c) => c.id) };
}

/** The payload of session.createSession: a title left blank takes the proposed one, a date left blank is not sent. */
export function payload(campaign: string, form: Form, sessions: readonly Session[]): Record<string, unknown> {
  const title = form.title.trim() || `Séance ${nextOrder(sessions)}`;
  return { campaign, title, ...(form.date === '' ? {} : { plannedOn: form.date }), present: form.present };
}

/** A key for one command: a browser that is not in a secure context has no `randomUUID`. */
export function newKey(): string {
  const crypto = globalThis.crypto as Crypto | undefined;
  return typeof crypto?.randomUUID === 'function' ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/** `POST /commands` for session.createSession, by identifier: the DM's own command. The same key is the same command: sent again, it creates nothing more. */
export function commandBody(dataCapability: string, body: Record<string, unknown>, idempotencyKey: string): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, caller: { by: 'dm', from: 'SessionList', roles: [] }, idempotencyKey };
}

const VIOLATIONS: Record<string, string> = {
  'present-only-pj': 'seuls les PJ de la campagne peuvent être présents',
  'oneshot-single-session': 'un one-shot n’a qu’une seule séance',
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
