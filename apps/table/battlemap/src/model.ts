// What the Micro-UI knows, with no DOM: the answers of the Capabilities as it
// reads them, what a player may see of them, the numbering of a « × n » drop,
// the commands a gesture becomes.

import { AGGREGATE, APPROVER, DATA_CAPABILITY_VERSION } from './ids.ts';
import { FEET_PER_HEX } from './hexgrid.ts';
import type { Character, Combat, Frame, Grid, Instance, MapInfo, Participant, Props, Shape, Size, Token, View } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const num = (value: unknown, fallback: number): number => (typeof value === 'number' && Number.isFinite(value) ? value : fallback);
const orNull = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);
const numbers = (value: unknown): Record<string, number> =>
  isRecord(value) ? Object.fromEntries(Object.entries(value).filter((e): e is [string, number] => typeof e[1] === 'number' && Number.isFinite(e[1]))) : {};
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);

/** The conditions a token can carry, with their badge. */
export const CONDITIONS: Readonly<Record<string, string>> = {
  'À terre': '↓',
  'Empoisonné': '☠',
  'Effrayé': '!',
  'Étourdi': '✦',
  'Agrippé': '✊',
  'Invisible': '?',
  'Concentration': '◎',
};

export const MAX_COUNT = 12;

/** The props, the pilot tab being asked for by `pilot` or by `?pilote=1` in the address (as the player stage reads it). */
export function parseProps(raw: Record<string, unknown>, search = ''): Props {
  // Absent, the DM's view; a value that is none of the three, the safest: the players'.
  const view: View = raw.view === undefined || raw.view === 'dm' ? 'dm' : raw.view === 'prep' ? 'prep' : 'player';
  return { instance: text(raw.instance), view, pilot: raw.pilot === true || new URLSearchParams(search).get('pilote') === '1', fill: raw.fill === true, campaign: text(raw.campaign) };
}

function token(value: Record<string, unknown>): Token {
  const name = text(value.name);
  const kind = value.kind === 'pnj' ? 'pnj' : 'pj';
  const hp = orNull(value.hp);
  return {
    id: text(value.id),
    char: text(value.char),
    kind,
    name,
    num: orNull(value.num),
    q: num(value.q, 0),
    r: num(value.r, 0),
    visible: value.visible !== false,
    hp,
    hpMax: orNull(value.hpMax),
    healthBand: text(value.healthBand ?? value.health_band ?? value.band) || null,
    states: texts(value.states),
    // dead-derived: an NPC is dead iff its current HP is 0, whatever else the answer says.
    dead: kind === 'pnj' && hp !== null ? hp <= 0 : value.dead === true,
    initials: text(value.initials) || name.slice(0, 2).toUpperCase(),
    color: text(value.color),
    portrait: value.portrait === true,
  };
}

/** A line of the order: the Capability answers `initiative`, the mock-up's store keeps `init`. */
function participant(value: Record<string, unknown>): Participant {
  return { id: text(value.id), init: num(value.initiative, num(value.init, 0)), dex: num(value.dex, 0) };
}

/** The combat of an instance, or null when none is under way; `current` is '' when the view hides whose turn it is. */
export function parseCombat(value: unknown): Combat | null {
  if (!isRecord(value) || !Array.isArray(value.order)) return null;
  const order = value.order.filter(isRecord).map(participant).filter((p) => p.id !== '');
  return order.length === 0 ? null : { round: Math.max(1, num(value.round, 1)), current: typeof value.current === 'string' ? value.current : '', order };
}

/** `table.getBattlemapInstance`: the tokens sit under `state` (or at the top), whichever side of the table asked. */
export function parseInstance(data: unknown): Instance | null {
  if (!isRecord(data)) return null;
  const state = isRecord(data.state) ? data.state : data;
  const tokens = Array.isArray(state.tokens) ? state.tokens.filter(isRecord).map(token) : [];
  return { id: text(data.id), battlemap: text(data.battlemap), session: text(data.session), label: text(data.label), roster: texts(data.roster), tokens, combat: parseCombat(state.combat), pjInitiative: numbers(data.pjInitiative) };
}

const GRID: Grid = { size: 32, ox: 0, oy: 0, orient: 'pointy', color: 'text', opacity: 0.45 };

function grid(value: unknown): Grid {
  const g = isRecord(value) ? value : {};
  return {
    size: num(g.size, 0) > 0 ? num(g.size, GRID.size) : GRID.size,
    ox: num(g.ox, GRID.ox),
    oy: num(g.oy, GRID.oy),
    orient: g.orient === 'flat' ? 'flat' : 'pointy',
    color: text(g.color) || GRID.color,
    opacity: Math.min(1, Math.max(0, num(g.opacity, GRID.opacity))),
  };
}

const SHAPES = new Set(['rect', 'ellipse', 'circle', 'path']);
const ATTRIBUTES = new Set(['x', 'y', 'width', 'height', 'cx', 'cy', 'rx', 'ry', 'r', 'd', 'class']);

/** The drawn terrain, kept to the few SVG shapes and attributes a map is drawn with. */
function shapes(value: unknown): Shape[] {
  if (!Array.isArray(value)) return [];
  return value.filter(isRecord).filter((s) => SHAPES.has(text(s.tag))).map((s) => ({
    tag: text(s.tag),
    attrs: Object.fromEntries(Object.entries(s).filter(([k, v]) => ATTRIBUTES.has(k) && (typeof v === 'string' || typeof v === 'number')).map(([k, v]) => [k, String(v)])),
  }));
}

/** `media.getBattlemap`: null for an unknown id, an illustration. */
export function parseMap(data: unknown): MapInfo | null {
  if (!isRecord(data)) return null;
  return { id: text(data.id), title: text(data.title), width: num(data.width, 800), height: num(data.height, 560), grid: grid(data.grid), terrain: shapes(data.terrain) };
}

/** The ability modifier of a score: floor((score - 10) / 2); 0 without a score. */
const modifier = (score: unknown): number => (typeof score === 'number' && Number.isFinite(score) ? Math.floor((score - 10) / 2) : 0);

/** `campaign.listCharacters`, by id. */
export function parseCharacters(data: unknown): Record<string, Character> {
  const out: Record<string, Character> = {};
  if (!Array.isArray(data)) return out;
  for (const entry of data) {
    if (!isRecord(entry) || text(entry.id) === '') continue;
    const name = text(entry.name);
    out[text(entry.id)] = {
      id: text(entry.id),
      name,
      tokenName: text(entry.tokenName) || name.split(' ')[0] || name,
      kind: entry.kind === 'pnj' ? 'pnj' : 'pj',
      initials: text(entry.initials) || name.slice(0, 2).toUpperCase(),
      color: text(entry.color),
      portrait: entry.portrait === true,
      dexMod: modifier(isRecord(entry.stats) ? entry.stats.DEX : undefined),
    };
  }
  return out;
}

/** players-see-band-only: hidden tokens do not exist for the players. */
export const shown = (tokens: readonly Token[], view: View): Token[] => (view === 'player' ? tokens.filter((t) => t.visible) : [...tokens]);

/** The initiative order a view reads: the players' has no hidden token, and a token revealed again stands at its former rank (hidden-keeps-rank). */
export function visibleOrder(combat: Combat, tokens: readonly Token[], view: View): Array<{ participant: Participant; token: Token }> {
  return combat.order.flatMap((participant) => {
    const found = tokens.find((t) => t.id === participant.id);
    return found && (view !== 'player' || found.visible) ? [{ participant, token: found }] : [];
  });
}

/** next-turn: the next living participant by rank; the round counts up when the order comes back to its start. */
export function turnAfter(combat: Combat, tokens: readonly Token[]): { current: string; round: number } {
  const { order } = combat;
  const from = Math.max(0, order.findIndex((p) => p.id === combat.current));
  for (let step = 1; step <= order.length; step += 1) {
    const at = (from + step) % order.length;
    const next = order[at];
    if (next && !tokens.find((t) => t.id === next.id)?.dead) return { current: next.id, round: at <= from ? combat.round + 1 : combat.round };
  }
  return { current: combat.current, round: combat.round };
}

/** An HP change kept within 0 and the maximum (hp-change-one-token: the caller applies it to the one token). */
export const hitPointsAfter = (hp: number, hpMax: number, delta: number): number => Math.max(0, Math.min(hpMax, hp + delta));

/** The order with `id` moved to `position` (clamped); the others keep their relative rank. */
export function reordered(order: readonly Participant[], id: string, position: number): Participant[] {
  const from = order.findIndex((p) => p.id === id);
  const moved = order[from];
  if (!moved) return [...order];
  const rest = order.filter((p) => p.id !== id);
  rest.splice(Math.max(0, Math.min(rest.length, position)), 0, moved);
  return rest;
}

/** The participant added to a running order, ranked as at the start: by initiative, then by DEX, then behind the ones already there. */
export function inserted(order: readonly Participant[], added: Participant): Participant[] {
  const at = order.findIndex((p) => p.init < added.init || (p.init === added.init && p.dex < added.dex));
  return [...order.slice(0, at < 0 ? order.length : at), added, ...(at < 0 ? [] : order.slice(at))];
}

/** The combat once `id` has left the order: the turn passes on if it was its, the combat ends with its last participant. */
export function without(combat: Combat, tokens: readonly Token[], id: string): Combat | null {
  const order = combat.order.filter((p) => p.id !== id);
  if (order.length === 0) return null;
  if (combat.current !== id) return { ...combat, order };
  // The leaver counts as gone: the turn goes to the next living participant by rank, and the round counts up if that is a lap.
  const next = turnAfter(combat, tokens.map((t) => (t.id === id ? { ...t, dead: true } : t)));
  return next.current === id ? { ...combat, current: order[0]?.id ?? '', order } : { round: next.round, current: next.current, order };
}

/** A d20 for an NPC, its initiative the roll plus the DEX modifier of its sheet. */
export const initiativeOf = (roll: number, dexMod: number): number => roll + dexMod;

/** HealthBand@1: told from the current and the maximum hit points and nothing else. */
export function bandOf(hp: number, hpMax: number): string {
  const current = Math.max(0, Math.min(hpMax, hp));
  return current === 0 ? 'mort' : current === hpMax ? 'indemne' : 2 * current > hpMax ? 'blessé' : 'en sang';
}

/** The band a player reads under an NPC: capitalised, never a figure. */
export const bandLabel = (band: string | null): string => (band === null || band === '' ? '' : band.charAt(0).toUpperCase() + band.slice(1));

/** multi-drop-numbering: the number the next token of `char` takes. */
export const nextNumber = (tokens: readonly Token[], char: string): number => Math.max(0, ...tokens.filter((t) => t.char === char).map((t) => t.num ?? 0)) + 1;

/** A PC has one token on a map: dropping it again moves it. */
export const existingToken = (tokens: readonly Token[], character: Character): Token | undefined => (character.kind === 'pj' ? tokens.find((t) => t.char === character.id) : undefined);

/** How many tokens a drop asks for, kept within 1 to MAX_COUNT. */
export const countOf = (raw: string): number => Math.max(1, Math.min(MAX_COUNT, Math.floor(Number(raw)) || 1));

/** What the drop dialog says of the numbers the new tokens will take. */
export function numbering(tokens: readonly Token[], character: Character, count: number): string {
  const first = nextNumber(tokens, character.id);
  if (count > 1) return `Numérotées de ${character.tokenName} ${first} à ${character.tokenName} ${first + count - 1}, renommables`;
  return first > 1 ? `Numérotée ${character.tokenName} ${first}, renommable` : 'Un seul token, renommable';
}

export const withCondition = (states: readonly string[], condition: string): string[] => (states.includes(condition) ? states.filter((s) => s !== condition) : [...states, condition]);

/** `POST /commands` for one DataCapability, by identifier, aimed at an aggregate. */
export function commandBody(dataCapability: string, aggregate: string, id: string | undefined, payload: Record<string, unknown>): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: id === undefined ? { aggregate } : { aggregate, id }, payload, caller: { by: 'dm', from: 'Battlemap', roles: [APPROVER] } };
}

/** A command of the table: aimed at the instance. */
export const tableCommand = (dataCapability: string, instance: string, payload: Record<string, unknown>): unknown => commandBody(dataCapability, AGGREGATE, instance, payload);

const VIOLATIONS: Record<string, string> = {
  'token-roster': 'un PJ absent de la séance, ou déjà sur la carte, ne peut pas être placé',
  'instance-isolated': 'ce token n’est pas sur cette carte',
  'base-coherent': 'cette carte n’est pas celle de la séance',
  'pj-hp-not-tracked': 'les PV d’un PJ ne sont pas suivis',
  'one-active-combat': 'il n’y a pas de combat en cours, ou il y en a déjà un',
  'npc-initiative-bounds': 'l’initiative saisie ou tirée est hors limites',
  'name-required': 'le nom ne peut pas être vide',
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
  return { status: text(result.status), violations: texts(result.violations) };
}

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}

/** « 1 hex = 5 pi », from HexGrid@1. */
export const SCALE = `1 hex = ${FEET_PER_HEX} pi`;

/** The frame as live.showOnPlayerView takes it: the zoom to 3 decimals, the pans whole map pixels. */
export const roundFrame = (frame: Frame): Frame => ({ zoom: Number(frame.zoom.toFixed(3)), panX: Math.round(frame.panX), panY: Math.round(frame.panY) });

/** Where the map's stage stands in a view: its scale, and the view's pixel of the stage's origin. */
export interface Placement {
  scale: number;
  x: number;
  y: number;
}

/** The scale at which the whole map fits the view. */
export const fitScale = (view: Size, map: Size): number => Math.min(view.width / map.width, view.height / map.height);

/** The placement of the stage in `view` for a frame: the map point the frame names at the view's centre, at the fit's scale times the zoom. */
export function placeStage(frame: Frame, view: Size, map: Size): Placement {
  const scale = fitScale(view, map) * frame.zoom;
  return { scale, x: view.width / 2 - (map.width / 2 + frame.panX) * scale, y: view.height / 2 - (map.height / 2 + frame.panY) * scale };
}

/** The frame a placement stands for in `view`: the inverse of `placeStage`. */
export function frameOf(placement: Placement, view: Size, map: Size): Frame {
  return { zoom: placement.scale / fitScale(view, map), panX: (view.width / 2 - placement.x) / placement.scale - map.width / 2, panY: (view.height / 2 - placement.y) / placement.scale - map.height / 2 };
}

/** The frame live.getDiffusion shows the players for `instance`: null when its base is another map or it carries no view. */
export function parseShownFrame(data: unknown, instance: string): Frame | null {
  if (!isRecord(data) || !isRecord(data.base) || data.base.kind !== 'battlemap' || data.base.id !== instance || !isRecord(data.view)) return null;
  const { zoom, panX, panY } = data.view;
  const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
  return finite(zoom) && zoom > 0 && finite(panX) && finite(panY) ? { zoom, panX, panY } : null;
}
