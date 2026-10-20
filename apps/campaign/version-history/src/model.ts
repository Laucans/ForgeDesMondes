// What the panel knows, with no DOM: the history as the Capability answers
// it, what the DM has not seen yet, and the command a restore becomes.

import { DATA_CAPABILITY_VERSION, ENTITY_AGGREGATE } from './ids.ts';
import type { Change, History, Version } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** A field's value as a diff shows it: text as it is, nothing as an empty string, anything else as JSON. */
const shown = (value: unknown): string => {
  if (value === null || value === undefined) return '';
  return typeof value === 'string' ? value : JSON.stringify(value);
};

function change(value: unknown): Change[] {
  return isRecord(value) && typeof value.field === 'string' ? [{ field: value.field, old: shown(value.old), new: shown(value.new) }] : [];
}

function version(value: unknown): Version[] {
  if (!isRecord(value) || typeof value.v !== 'number') return [];
  return [
    {
      v: value.v,
      date: text(value.date),
      origin: text(value.origin),
      source: text(value.source),
      restoredFrom: typeof value.restoredFrom === 'number' ? value.restoredFrom : null,
      seen: value.seen !== false,
      changes: Array.isArray(value.changes) ? value.changes.flatMap(change) : [],
    },
  ];
}

/** The Capability's answer, however little of it there is: an unknown element (null) has an empty history, not an error. */
export function parseHistory(data: unknown): History {
  const answer = isRecord(data) ? data : {};
  const versions = Array.isArray(answer.versions) ? answer.versions.flatMap(version) : [];
  return { name: text(answer.name), versions: versions.sort((a, b) => b.v - a.v) };
}

/** Whether a version has one before it to go back to: the first, the creation, has not. */
export const hasPrevious = (history: History, v: Version): boolean => history.versions.some((other) => other.v === v.v - 1);

/** The number of the newest version, 0 for an element never saved. */
export const topVersion = (history: History): number => history.versions[0]?.v ?? 0;

/**
 * Up to which version the DM has seen the history when it is first read: every version
 * before the oldest one the Capability itself calls unseen, else all of them.
 */
export function seenUpTo(history: History): number {
  const unseen = history.versions.filter((v) => !v.seen).map((v) => v.v);
  return unseen.length === 0 ? topVersion(history) : Math.min(...unseen) - 1;
}

/** What the DM has not looked at: a version after `upTo` that someone other than the DM made. */
export const unseen = (history: History, upTo: number): Version[] => history.versions.filter((v) => v.v > upTo && v.origin !== 'manuelle');

const SOURCES: Record<string, string> = {
  dm: 'Fiche modifiée à la main',
  ia: 'Génération par l’IA',
  dndbeyond: 'Synchronisation D&D Beyond',
  brief: 'Brief oral',
};

/** A version's source in words: a restoration names the version it put back. */
export function sourceLabel(v: Version): string {
  if (v.source === 'restauration') return v.restoredFrom === null ? 'Retour à une version antérieure' : `Retour à la v${v.restoredFrom}`;
  return SOURCES[v.source] ?? v.source;
}

/** How a version's origin is badged: its label and the kit Badge's tone. */
export function originBadge(origin: string): { label: string; tone: string | null } {
  switch (origin) {
    case 'ia':
      return { label: 'IA', tone: 'accent' };
    case 'manuelle':
      return { label: 'Manuelle', tone: 'primary' };
    case 'dndbeyond':
      return { label: 'D&D Beyond', tone: 'success' };
    case 'import':
      return { label: 'Import de notes', tone: null };
    default:
      return { label: origin, tone: null };
  }
}

/** What a restore asks for: one version, and — to undo only some changes of the version after it — which of that version's changes, by position in its diff. */
export interface Restore {
  version: number;
  changes?: number[];
}

/**
 * `POST /commands` restoring an element, by identifier. Going back to a version names it and
 * no field (`restore-exact`); undoing a change names the version before it and the changes.
 * The key makes one click one command, whatever the network does with it.
 */
export function restoreBody(dataCapability: string, entity: string, restore: Restore, idempotencyKey: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    target: { aggregate: ENTITY_AGGREGATE, id: entity },
    payload: restore.changes === undefined ? { version: restore.version } : { version: restore.version, changes: restore.changes },
    idempotencyKey,
    caller: { by: 'dm', from: 'VersionHistory', roles: [] as string[] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'alias-unique-in-campaign': 'un alias de cette version appartient déjà à un autre élément',
  'not-found': 'cette version n’existe pas pour cette fiche',
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
