// What the sheet knows, with no DOM: the answers of the Capabilities, the
// draft the DM edits against the sheet the server holds, the commands a click
// becomes and the diff of what the AI or D&D Beyond changed.

import { APPROVER, DATA_CAPABILITY_VERSION, ENTITY_AGGREGATE, NEW_PREFIX } from './ids.ts';
import { ABILITIES, type Ability, type Change, type Crop, type Entity, type Feature, type Kind, type Media, type Player, type Target } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const records = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? value.filter(isRecord) : []);
const whole = (value: unknown, otherwise: number): number => (typeof value === 'number' && Number.isFinite(value) ? value : otherwise);

export const KINDS: readonly Kind[] = ['pj', 'pnj', 'lieu', 'evenement'];
export const isKind = (value: unknown): value is Kind => typeof value === 'string' && (KINDS as readonly string[]).includes(value);
/** A PC or an NPC has a sheet; a place or an event has none (character-is-pj-or-pnj). */
export const isCharacter = (kind: Kind): boolean => kind === 'pj' || kind === 'pnj';

/** The score every ability starts from. */
export const BASE_SCORE = 10;
export const DEFAULT_CROP: Crop = { zoom: 100, x: 50, y: 50 };
export const DEFAULT_COLOR = 'accent';

/** AbilityModifier@1: `floor((score - 10) / 2)`, toward negative infinity — 9 gives -1, 18 gives +4. Derived, never typed (dex-modifier-derived). */
export const modifier = (score: number): number => Math.floor((score - BASE_SCORE) / 2);

/** The modifier as the sheet writes it: « +4 », « −1 ». */
export function signed(score: number): string {
  const m = modifier(score);
  return m < 0 ? `−${Math.abs(m)}` : `+${m}`;
}

/** TokenAppearance@1: the first letters of the first two words of the name, or the first two letters of a one-word name, in capitals. */
export function initialsOf(name: string): string {
  const words = name.split(/\s+/).filter((word) => word !== '');
  const letters = words.length === 0 ? [] : words.length === 1 ? [...(words[0] ?? '')].slice(0, 2) : [...(words[0] ?? '')].slice(0, 1).concat([...(words[1] ?? '')].slice(0, 1));
  return letters.join('').toUpperCase().slice(0, 2);
}

/** Whether the sheet has a portrait: the one just picked, else the one the server holds. */
const hasPortrait = (entity: Pick<Entity, 'portrait' | 'portraitMedia'>): boolean => (entity.portraitMedia === undefined ? entity.portrait : entity.portraitMedia !== null);

/** default-token (TokenAppearance@1): with neither a portrait nor a token image, the token is the initials on the colour. */
export function tokenHasImage(entity: Pick<Entity, 'portrait' | 'tokenImage' | 'portraitMedia' | 'tokenMedia'>): boolean {
  const token = entity.tokenMedia === undefined ? entity.tokenImage : entity.tokenMedia !== null;
  return hasPortrait(entity) || token;
}

/** A name of a colour of the design system, else the default: it ends up in a `var(--color-…)`. */
export const colorName = (color: string): string => (/^[a-z][a-z-]*$/.test(color) ? color : DEFAULT_COLOR);

function scores(value: unknown): Record<Ability, number> {
  const given = isRecord(value) ? value : {};
  return Object.fromEntries(ABILITIES.map((ability) => [ability, whole(given[ability], BASE_SCORE)])) as Record<Ability, number>;
}

function crop(value: unknown): Crop {
  const given = isRecord(value) ? value : {};
  return { zoom: whole(given.zoom, DEFAULT_CROP.zoom), x: whole(given.x, DEFAULT_CROP.x), y: whole(given.y, DEFAULT_CROP.y) };
}

/** The sheet of `campaign.getEntity`; null for an unknown element (the Capability answers null) or anything that is not one. */
export function parseEntity(data: unknown): Entity | null {
  if (!isRecord(data) || !isKind(data.kind) || text(data.id) === '') return null;
  const ddb = isRecord(data.ddb) && text(data.ddb.url) !== '' ? { url: text(data.ddb.url), syncedAt: typeof data.ddb.syncedAt === 'string' ? data.ddb.syncedAt : null } : null;
  const features: Feature[] = records(data.features).map((feature) => ({ name: text(feature.name), text: text(feature.text) }));
  return {
    id: text(data.id),
    kind: data.kind,
    name: text(data.name),
    aliases: texts(data.aliases),
    description: text(data.description),
    notes: text(data.notes),
    player: text(data.player),
    prompt: text(data.prompt),
    background: text(data.background),
    behavior: text(data.behavior),
    stats: scores(data.stats),
    ca: whole(data.ca, BASE_SCORE),
    pvMax: whole(data.pvMax, 8),
    features,
    color: isCharacter(data.kind) ? colorName(text(data.color)) : '',
    portraitMedia: undefined,
    tokenMedia: undefined,
    portrait: data.portrait === true,
    tokenImage: data.tokenImage === true,
    crop: crop(data.crop),
    ddb,
  };
}

/** A sheet to create: nothing saved yet, a token colour to fall back on. */
export function freshEntity(kind: Kind, name = ''): Entity {
  return {
    id: '',
    kind,
    name,
    aliases: [],
    description: '',
    notes: '',
    player: '',
    prompt: '',
    background: '',
    behavior: '',
    stats: scores(undefined),
    ca: BASE_SCORE,
    pvMax: 8,
    features: [],
    color: isCharacter(kind) ? DEFAULT_COLOR : '',
    portraitMedia: undefined,
    tokenMedia: undefined,
    portrait: false,
    tokenImage: false,
    crop: { ...DEFAULT_CROP },
    ddb: null,
  };
}

/** The players of the campaign. */
export const parsePlayers = (data: unknown): Player[] =>
  records(data)
    .filter((player) => text(player.id) !== '')
    .map((player) => ({ id: text(player.id), name: text(player.name) || text(player.id) }));

/** The illustrations of the campaign's library: what a portrait or a token can be. */
export const parseIllustrations = (data: unknown): Media[] =>
  records(isRecord(data) ? data.illustrations : [])
    .filter((media) => text(media.id) !== '')
    .map((media) => ({ id: text(media.id), title: text(media.title) || text(media.id) }));

/** What the prop asks for: an element by its id, or a sheet to create (`new:pnj`); null when it asks for nothing. */
export function parseTarget(prop: unknown): Target | null {
  if (typeof prop !== 'string' || prop === '') return null;
  if (!prop.startsWith(NEW_PREFIX)) return { id: prop };
  const kind = prop.slice(NEW_PREFIX.length);
  return isKind(kind) ? { fresh: kind } : null;
}

/** The prop that asks for `target`. */
export const targetProp = (target: Target): string => ('id' in target ? target.id : `${NEW_PREFIX}${target.fresh}`);

export const sameTarget = (a: Target | null, b: Target | null): boolean => (a === null || b === null ? a === b : targetProp(a) === targetProp(b));

/** The aliases as typed: separated by commas, blanks dropped. */
export const parseAliases = (typed: string): string[] =>
  typed
    .split(',')
    .map((alias) => alias.trim())
    .filter((alias) => alias !== '');

/** The fields the DM edits, and the AI or D&D Beyond may change. */
const FIELDS = ['name', 'aliases', 'description', 'notes', 'player', 'prompt', 'background', 'behavior', 'ca', 'pvMax', 'features', 'color', 'crop'] as const;
type Field = (typeof FIELDS)[number];

/** What the sheet is compared by: everything the DM edits. */
const comparable = (entity: Entity): unknown => ({
  kind: entity.kind,
  ...Object.fromEntries(FIELDS.map((field) => [field, entity[field]])),
  stats: entity.stats,
  portraitMedia: entity.portraitMedia,
  tokenMedia: entity.tokenMedia,
});

/** explicit-save-only: the sheet is modified when the draft is not the sheet the server holds. A new sheet is modified as soon as it has been touched. */
export function isDirty(draft: Entity, stored: Entity): boolean {
  return JSON.stringify(comparable(draft)) !== JSON.stringify(comparable(stored));
}

/** `POST /commands` for one DataCapability, by identifier. Each is the DM's own act on their sheet, whose identity and versions are critical: the DM approves it by asking for it. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>, target: { aggregate: string; id?: string }, idempotencyKey: string): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    target,
    idempotencyKey,
    caller: { by: 'dm', from: 'EntityEditor', roles: [APPROVER] },
  };
}

/** The element a command targets: its id once it has one. */
export const entityTarget = (id: string): { aggregate: string; id?: string } => (id === '' ? { aggregate: ENTITY_AGGREGATE } : { aggregate: ENTITY_AGGREGATE, id });

/** A uuid for an element to create: the sheet names it, so it can open the element it saved. */
export function newId(): string {
  const crypto = globalThis.crypto as Crypto | undefined;
  if (typeof crypto?.randomUUID === 'function') return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  if (typeof crypto?.getRandomValues === 'function') crypto.getRandomValues(bytes);
  else bytes.forEach((_, i) => (bytes[i] = Math.floor(Math.random() * 256)));
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** The payload of `campaign.saveEntity`: the fields the DM edits, under `entity`; a DEX modifier is never sent (AbilityModifier@1). A new element names its campaign. */
export function savePayload(entity: Entity, campaign: string, isNew: boolean): Record<string, unknown> {
  const sent: Record<string, unknown> = {
    id: entity.id,
    kind: entity.kind,
    name: entity.name.trim(),
    aliases: entity.aliases,
    description: entity.description,
    notes: entity.notes,
  };
  if (isCharacter(entity.kind)) {
    sent.player = entity.kind === 'pj' && entity.player !== '' ? entity.player : null;
    sent.prompt = entity.prompt;
    sent.background = entity.background;
    sent.behavior = entity.behavior;
    sent.stats = entity.stats;
    sent.ca = entity.ca;
    sent.pvMax = entity.pvMax;
    sent.features = entity.features;
    // default-token (TokenAppearance@1): a crop is a crop of the portrait, so a sheet with none keeps none.
    sent.crop = hasPortrait(entity) ? entity.crop : null;
    sent.color = entity.color;
    if (entity.portraitMedia !== undefined) sent.portrait = entity.portraitMedia;
    if (entity.tokenMedia !== undefined) sent.token = entity.tokenMedia;
  }
  return isNew ? { campaign, entity: sent } : { entity: sent };
}

const LABELS: Record<Field, string> = {
  name: 'Nom',
  aliases: 'Alias',
  description: 'Description',
  notes: 'Notes du MJ',
  player: 'Joueur',
  prompt: 'Prompt initial',
  background: 'Background',
  behavior: 'Comportement',
  ca: 'CA',
  pvMax: 'PV max',
  features: 'Sorts et capacités',
  color: 'Couleur du token',
  crop: 'Recadrage du token',
};

const shown = (field: Field | Ability, entity: Entity): string => {
  if (field === 'aliases') return entity.aliases.join(', ');
  if (field === 'features') return entity.features.map((feature) => feature.name).join(', ');
  if (field === 'crop') return `${entity.crop.zoom} % · ${entity.crop.x} / ${entity.crop.y}`;
  if ((ABILITIES as readonly string[]).includes(field)) return String(entity.stats[field as Ability]);
  const value = entity[field as Field];
  return typeof value === 'string' || typeof value === 'number' ? String(value) : '';
};

/** The fields that moved between two sheets, as the diff of the history words them. */
export function changesBetween(before: Entity, after: Entity): Change[] {
  const moved = (field: Field | Ability, a: unknown, b: unknown): Change[] => (JSON.stringify(a) === JSON.stringify(b) ? [] : [{ field: field in LABELS ? LABELS[field as Field] : field, old: shown(field, before), new: shown(field, after) }]);
  return [...FIELDS.flatMap((field) => moved(field, before[field], after[field])), ...ABILITIES.flatMap((ability) => moved(ability, before.stats[ability], after.stats[ability]))];
}

/** What the AI or D&D Beyond changed in the stored sheet is put on the draft; what the DM typed elsewhere stays, unsaved. */
export function overlay(draft: Entity, before: Entity, after: Entity): Entity {
  const next: Entity = structuredClone(draft);
  const set = <K extends keyof Entity>(key: K): void => {
    next[key] = structuredClone(after[key]);
  };
  for (const field of FIELDS) if (JSON.stringify(before[field]) !== JSON.stringify(after[field])) set(field);
  for (const ability of ABILITIES) if (before.stats[ability] !== after.stats[ability]) next.stats[ability] = after.stats[ability];
  // Facts of the read, not edits.
  set('portrait');
  set('tokenImage');
  set('ddb');
  return next;
}

const VIOLATIONS: Record<string, string> = {
  'not-found': 'l’élément ou sa demande est introuvable',
  'alias-unique-in-campaign': 'un nom ou un alias est déjà celui d’un autre élément de la campagne',
  'character-is-pj-or-pnj': 'seul un PJ a un joueur, et seul un PJ ou un PNJ a une fiche',
  'dex-modifier-derived': 'le modificateur se calcule, il ne s’écrit pas',
  'dndbeyond-resync-scope': 'la resynchronisation ne touche que les stats, les PV et les sorts',
  'proposal-fills-sheet-fields': 'la proposition ne remplit pas les champs de la fiche',
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

/** The variables the campaign's lists are read with: its id, when the screen knows it. */
export const campaignVariables = (campaign: string): Record<string, unknown> => (campaign === '' ? {} : { campaign });
