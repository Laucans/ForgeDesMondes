// What the Micro-UI knows, with no DOM: the brief as the Capability answers
// it, the command the screen becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Brief, Missing, Recognized, Scene } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const count = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

const list = <T>(value: unknown, read: (item: unknown) => T | null): T[] => (Array.isArray(value) ? value.map(read).filter((item): item is T => item !== null) : []);

function readScene(value: unknown): Scene | null {
  if (!isRecord(value) || text(value.title) === '') return null;
  return { order: count(value.order), title: text(value.title), links: list(value.links, (link) => (typeof link === 'string' && link !== '' ? link : null)) };
}

function readRecognized(value: unknown): Recognized | null {
  if (!isRecord(value) || text(value.id) === '') return null;
  return { id: text(value.id), type: text(value.type), name: text(value.name), heard: text(value.heard) || null, updated: value.updated === true, change: text(value.change) };
}

function readMissing(value: unknown): Missing | null {
  if (!isRecord(value) || text(value.id) === '') return null;
  return { id: text(value.id), type: text(value.type), name: text(value.name) };
}

/** The Capability's answer, however little of it there is: a session with no brief is the same shape, empty. */
export function parseBrief(data: unknown): Brief {
  const answer = isRecord(data) ? data : {};
  const result = isRecord(answer.result) ? answer.result : {};
  return {
    lastRun: text(answer.lastRun) || null,
    duration: text(answer.duration) || null,
    transcript: list(answer.transcript, (line) => (typeof line === 'string' && line !== '' ? line : null)),
    // The scenes are shown in their order, whatever order the answer lists them in.
    scenes: list(result.scenes, readScene).sort((a, b) => a.order - b.order),
    recognized: list(result.recognized, readRecognized),
    missing: list(result.missing, readMissing),
  };
}

/** `POST /commands` for one DataCapability, by identifier: the DM's own. Only text goes. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload, caller: { by: 'dm', from: 'OralBrief', roles: [] } };
}

const VIOLATIONS: Record<string, string> = {
  'brief-no-auto-creation': 'le brief ne crée aucun élément',
  'text-only-no-audio': 'seul du texte est accepté',
  'scene-order-contiguous': 'les scènes doivent rester numérotées sans trou',
  'scene-links-same-campaign': 'un élément lié doit être de la campagne de la séance',
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

/** « Dernier brief : 14 octobre à 19:31 · 04:12 », in the DM's own time; '' while no brief has been analysed. The Capability dates in UTC, with no zone mark. */
export function lastRunLabel(lastRun: string | null, duration: string | null): string {
  if (lastRun === null) return '';
  const parsed = new Date(/(Z|[+-]\d\d:?\d\d)$/.test(lastRun) ? lastRun : `${lastRun}Z`);
  const when = Number.isNaN(parsed.getTime()) ? lastRun : parsed.toLocaleString('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' });
  return `Dernier brief : ${when}${duration === null ? '' : ` · ${duration}`}`;
}
