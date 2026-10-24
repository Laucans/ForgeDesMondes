// What the Micro-UI knows, with no DOM: the entries as the Capability answers
// them, the proposal as the created element holds it, the commands the screen
// becomes.

import { DATA_CAPABILITY_VERSION, SESSION_AGGREGATE } from './ids.ts';
import type { Ability, Item, Proposal } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const figure = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

function readItem(value: unknown): Item | null {
  if (!isRecord(value) || text(value.id) === '') return null;
  const entity = text(value.entity) === '' ? null : text(value.entity);
  return {
    id: text(value.id),
    type: text(value.type),
    name: text(value.name),
    session: text(value.session) === '' ? null : text(value.session),
    sessionTitle: text(value.sessionTitle),
    excerpt: text(value.excerpt),
    // to-create-leaves-once-created: an entry is created if and only if its element is set.
    status: value.status === 'created' && entity !== null ? 'created' : 'todo',
    entity: value.status === 'created' ? entity : null,
  };
}

/** The Capability's answer — `{ items }`, or the bare list — however little of it there is. */
export function parseItems(data: unknown): Item[] {
  const list = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.items) ? data.items : [];
  return list.map(readItem).filter((item): item is Item => item !== null);
}

/** The entries to show: all of the campaign's, or those of one session. */
export const inSession = (items: readonly Item[], session: string): Item[] => (session === '' ? [...items] : items.filter((item) => item.session === session));

export const remaining = (items: readonly Item[]): number => items.filter((item) => item.status === 'todo').length;

/** « N à créer · M créée(s) ». */
export const countLabel = (items: readonly Item[]): string => `${remaining(items)} à créer · ${items.length - remaining(items)} créée(s)`;

const ABILITIES = ['FOR', 'DEX', 'CON', 'INT', 'SAG', 'CHA'];

function readSheet(entity: Record<string, unknown>): Proposal['sheet'] {
  if (!isRecord(entity.stats)) return null;
  const stats = entity.stats;
  const modifiers = isRecord(entity.modifiers) ? entity.modifiers : {};
  const abilities: Ability[] = ABILITIES.map((key) => ({ key, score: figure(stats[key]), modifier: figure(modifiers[key]) }));
  const features = Array.isArray(entity.features)
    ? entity.features.filter(isRecord).map((feature) => ({ name: text(feature.name), text: text(feature.text) }))
    : [];
  return { ca: figure(entity.ca), pvMax: figure(entity.pvMax), abilities, features };
}

/** The element `campaign.getEntity` answers, read as the proposal; null when there is no element. */
export function parseProposal(data: unknown): Proposal | null {
  if (!isRecord(data) || text(data.id) === '') return null;
  return {
    name: text(data.name),
    description: text(data.description),
    aliases: Array.isArray(data.aliases) ? data.aliases.filter((alias): alias is string => typeof alias === 'string' && alias !== '') : [],
    background: text(data.background),
    behavior: text(data.behavior),
    sheet: readSheet(data),
  };
}

/** A modifier as the statblock writes it: « +2 », « −1 ». */
export const signed = (modifier: number): string => (modifier < 0 ? `−${Math.abs(modifier)}` : `+${modifier}`);

/** The refinement the DM wrote; none when it is blank. */
export const refinementOf = (typed: string): string | null => (typed.trim() === '' ? null : typed.trim());

/** `POST /commands` for the creation, by identifier: the refinement is sent only when there is one. */
export function commandBody(dataCapability: string, item: Item, refinement: string | null): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload: { entry: item.id, ...(refinement === null ? {} : { refinement }) },
    ...(item.session === null ? {} : { target: { aggregate: SESSION_AGGREGATE, id: item.session } }),
    caller: { by: 'dm', from: 'ToCreateList', roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'to-create-leaves-once-created': 'cet élément est déjà créé',
  'not-found': 'cet élément n’est plus dans la liste',
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
  const violations = Array.isArray(result.violations) ? result.violations.filter((v): v is string => typeof v === 'string') : [];
  return { status: text(result.status), violations };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
