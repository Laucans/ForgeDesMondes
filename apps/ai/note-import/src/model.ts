// What the Micro-UI knows, with no DOM: the notes as the Capability answers
// them, the commands the screen becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Import, Notes, Proposal } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const count = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

function readImport(value: unknown): Import | null {
  if (!isRecord(value) || text(value.id) === '') return null;
  return { id: text(value.id), date: text(value.date), title: text(value.title), chars: count(value.chars), proposed: count(value.proposed), created: count(value.created) };
}

function readProposal(value: unknown): Proposal | null {
  if (!isRecord(value) || text(value.id) === '') return null;
  const existing = typeof value.existing === 'string' && value.existing !== '' ? value.existing : null;
  // A probable duplicate starts unchecked, whatever the answer says.
  return { id: text(value.id), type: text(value.type), name: text(value.name), excerpt: text(value.excerpt), existing, checked: existing === null && value.checked !== false };
}

const list = <T>(value: unknown, read: (item: unknown) => T | null): T[] => (Array.isArray(value) ? value.map(read).filter((item): item is T => item !== null) : []);

/** The Capability's answer — `{ imports, proposals }`, or the bare list of imports — however little of it there is. */
export function parseNotes(data: unknown): Notes {
  if (Array.isArray(data)) return { imports: list(data, readImport), proposals: [] };
  const answer = isRecord(data) ? data : {};
  return { imports: list(answer.imports, readImport), proposals: list(answer.proposals, readProposal) };
}

/** The proposals the DM has not already settled in this screen. */
export const pending = (notes: Notes, settled: ReadonlySet<string>): Proposal[] => notes.proposals.filter((proposal) => !settled.has(proposal.id));

/** « Créer la sélection (n) ». */
export const createLabel = (checked: number): string => `Créer la sélection (${checked})`;

/** `POST /commands` for one DataCapability, by identifier: the DM's own. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload, caller: { by: 'dm', from: 'NoteImport', roles: [] } };
}

const VIOLATIONS: Record<string, string> = {
  'text-only-payload': 'seul du texte est accepté',
  'no-creation-before-validation': 'rien ne se crée avant la validation',
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
