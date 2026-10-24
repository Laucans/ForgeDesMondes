// What the Micro-UI knows, with no DOM: the suggestions as the Capability
// answers them, the pile and the history they make (pile-and-history-derived),
// the commands a click becomes and how a command is followed.

import { DATA_CAPABILITY_VERSION, MICRO_UI } from './ids.ts';
import type { Kind, Props, Suggestion, Suggestions } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** How many suggestions the pile holds. */
export const PILE = 5;

export function parseProps(raw: Record<string, unknown>): Props {
  return { campaign: text(raw.campaign) };
}

function parseSuggestion(value: unknown): Suggestion | null {
  if (!isRecord(value)) return null;
  const id = text(value.id);
  if (id === '') return null;
  const kind: Kind = value.kind === 'sound' ? 'sound' : 'entity';
  return { id, kind, type: kind === 'sound' ? '' : text(value.type), name: text(value.name), via: text(value.via), detectedAt: text(value.detectedAt) };
}

const list = (value: unknown): Suggestion[] => (Array.isArray(value) ? value.map(parseSuggestion).filter((s): s is Suggestion => s !== null) : []);

/**
 * The answer, however little of it there is: `{ suggestions, history }`, or a bare list. The history is
 * every suggestion by detection time descending, the pile its first 5: what the answer lists is held to
 * that here, so a pile never shows more than the 5 most recent. An answer that gives no time for one of
 * them keeps the order it came in.
 */
export function parseSuggestions(data: unknown): Suggestions {
  const answer = isRecord(data) ? data : {};
  const all = Array.isArray(data) ? list(data) : list(answer.history).length > 0 ? list(answer.history) : list(answer.suggestions);
  const times = all.map((s) => Date.parse(s.detectedAt));
  const history = times.every((t) => !Number.isNaN(t)) ? all.map((s, i) => ({ s, t: times[i] ?? 0 })).sort((a, b) => b.t - a.t).map(({ s }) => s) : all;
  return { stack: history.slice(0, PILE), history };
}

/** The hour and minute a suggestion was detected at, as the DM reads them; empty when the answer gives no time. */
export function timeOf(detectedAt: string): string {
  const at = Date.parse(detectedAt);
  return Number.isNaN(at) ? '' : new Date(at).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
}

/** The kind of element the host is told to open, from the type the suggestion carries. */
export function hostType(type: string): 'place' | 'event' | 'character' {
  const folded = type.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  if (folded === 'lieu' || folded === 'place') return 'place';
  if (folded === 'evenement' || folded === 'event') return 'event';
  return 'character';
}

/** `POST /commands` for one DataCapability, by identifier. The DM's own, from this Micro-UI. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, targetId?: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    ...(targetId === undefined ? {} : { target: { id: targetId } }),
    caller: { by: 'dm', from: MICRO_UI, roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'status-lifecycle': 'la séance est terminée',
  'dm-side-only': 'réservé à la vue du MJ',
  'not-found': 'introuvable',
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
  const violations = Array.isArray(result.violations) ? result.violations.filter((v): v is string => typeof v === 'string') : [];
  return { status: text(result.status), violations };
}

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
