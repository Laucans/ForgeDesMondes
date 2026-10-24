// What the sheet knows, with no DOM: the Capabilities' answers reduced to
// what it draws, the illustration of an element, the commands a send becomes.

import { AGGREGATE, DATA_CAPABILITY_VERSION } from './ids.ts';
import { ABILITIES, type Ability, type ElementType, type Entity, type Feature, type Illustration, type Media, type Props, type Ref, type Target } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const num = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

const TYPES: readonly ElementType[] = ['character', 'place', 'event', 'illustration'];

/** The element a screen or an `open` event names; anything else is no element. */
export function parseTarget(raw: Record<string, unknown>): Target {
  return { type: TYPES.find((t) => t === raw.type) ?? '', id: text(raw.id) };
}

export function parseProps(raw: Record<string, unknown>): Props {
  return { ...parseTarget(raw), campaign: text(raw.campaign) };
}

const KINDS = new Map<string, Exclude<ElementType, 'illustration'>>([
  ['pj', 'character'],
  ['pnj', 'character'],
  ['lieu', 'place'],
  ['evenement', 'event'],
]);

function scores(value: unknown): Record<Ability, number | null> {
  const given = isRecord(value) ? value : {};
  return Object.fromEntries(ABILITIES.map((k) => [k, num(given[k])])) as Record<Ability, number | null>;
}

function features(value: unknown): Feature[] {
  if (!Array.isArray(value)) return [];
  return (value as unknown[]).filter(isRecord).map((f) => ({ name: text(f.name), text: text(f.text) })).filter((f) => f.name !== '' || f.text !== '');
}

/** A character met in a place, as the answer lists it: an id, or an id with its name. */
export function parseRefs(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const ids = (value as unknown[]).map((c) => (isRecord(c) ? text(c.id) : text(c))).filter((id) => id !== '');
  return [...new Set(ids)];
}

/** campaign.getEntity's answer for the element asked for; null when it has no answer. */
export function parseEntity(data: unknown, asked: Target): Entity | null {
  if (!isRecord(data) || text(data.id) === '') return null;
  const kind = text(data.kind);
  const type = KINDS.get(kind) ?? (asked.type === '' || asked.type === 'illustration' ? (isRecord(data.stats) ? 'character' : 'place') : asked.type);
  const name = text(data.name);
  return {
    id: text(data.id),
    name,
    aliases: Array.isArray(data.aliases) ? (data.aliases as unknown[]).filter((a): a is string => typeof a === 'string' && a !== '') : [],
    type,
    pc: kind === 'pj' || (kind === '' && text(data.player) !== ''),
    description: text(data.description),
    notes: text(data.notes),
    behavior: text(data.behavior),
    background: text(data.background),
    creature: text(data.type),
    stats: scores(data.stats),
    modifiers: scores(data.modifiers),
    ca: num(data.ca),
    pvMax: num(data.pvMax),
    speed: typeof data.speed === 'number' ? String(data.speed) : text(data.speed),
    features: features(data.features),
    art: text(data.art),
    initials: text(data.initials) || name.charAt(0),
    illustration: text(data.illustration),
    characters: parseRefs(data.characters),
    campaign: text(data.campaignId) || text(data.campaign),
  };
}

/** A character of a place, as its own sheet names it; null when it has no sheet to open. */
export function refOf(data: unknown): Ref | null {
  const entity = parseEntity(data, { type: 'character', id: '' });
  return entity === null || entity.name === '' ? null : { id: entity.id, name: entity.name, pc: entity.pc };
}

/** media.listMedia's answer, reduced to the illustrations. */
export function parseMedia(data: unknown): Media {
  const answer = isRecord(data) ? data : {};
  const illustrations: Illustration[] = [];
  if (Array.isArray(answer.illustrations)) {
    for (const entry of answer.illustrations as unknown[]) {
      if (isRecord(entry) && text(entry.id) !== '') {
        illustrations.push({ id: text(entry.id), title: text(entry.title), art: text(entry.art), glyph: text(entry.glyph), place: text(entry.place), file: text(entry.file), size: text(entry.size) });
      }
    }
  }
  return { illustrations };
}

export const illustrationOf = (media: Media, id: string): Illustration | undefined => media.illustrations.find((i) => i.id === id);

/** The illustration of an element: the one its answer links, else, for a place, the one the library attaches to it. */
export function illustrationFor(entity: Entity, media: Media): Illustration | undefined {
  const linked = illustrationOf(media, entity.illustration);
  if (linked !== undefined || entity.type !== 'place') return linked;
  return media.illustrations.find((i) => i.place === entity.id);
}

/** An ability's modifier as it is written: the answer's, else computed from the score (AbilityModifier@1); empty without a score. */
export function modifierText(score: number | null, given: number | null): string {
  if (score === null && given === null) return '';
  const value = given ?? Math.floor(((score ?? 10) - 10) / 2);
  return value < 0 ? `−${Math.abs(value)}` : `+${value}`;
}

/** `POST /commands` for one DataCapability, by identifier, aimed at the diffusion. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: { aggregate: AGGREGATE }, payload, caller: { by: 'dm', from: 'TableSheet', roles: [] } };
}

const VIOLATIONS: Record<string, string> = {
  'base-coherent': 'l’illustration n’est pas dans la médiathèque de la campagne',
  'window-proportional': 'la fenêtre doit garder les proportions de l’image',
  'window-fields-together': 'la fenêtre ne peut pas s’ouvrir',
  'single-diffusion-source': 'la diffusion a changé entre-temps',
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
