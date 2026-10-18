// What the stage knows, with no DOM: the Capabilities' answers reduced to
// what a player may see, the window's geometry, the commands a drag becomes.
// Nothing outside the fields read here ever reaches the screen
// (`players-see-band-only`): no HP figure, no note, no hidden token.

import { AGGREGATE, DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Base, BaseKind, Box, Campaign, Diffusion, Framing, Illustration, Media, Props, Stage, Window } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const num = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

/** The smallest window, as a fraction of the stage's width. */
export const MIN_WIDTH = 0.1;

const KINDS: readonly BaseKind[] = ['cover', 'illustration', 'battlemap'];
const COVER: Base = { kind: 'cover', id: '' };

function base(value: unknown): Base {
  if (!isRecord(value)) return COVER;
  const kind = KINDS.find((k) => k === value.kind);
  return kind === undefined ? COVER : { kind, id: text(value.id) };
}

function windowOf(value: unknown): Window | null {
  if (!isRecord(value)) return null;
  const [x, y, w] = [num(value.x), num(value.y), num(value.w)];
  const media = text(value.media) || text(value.id);
  if (media === '' || x === null || y === null || w === null || w <= 0) return null;
  // A row off the stage is brought back onto it: whole at 100%, never drawn beyond.
  const width = Math.min(w, 1);
  return { media, x: Math.max(0, Math.min(x, 1 - width)), y: Math.max(0, Math.min(y, 1)), w: width };
}

function framing(value: unknown): Framing | null {
  if (!isRecord(value)) return null;
  const [zoom, panX, panY] = [num(value.zoom ?? value.z), num(value.panX ?? value.px), num(value.panY ?? value.py)];
  return zoom === null || panX === null || panY === null ? null : { zoom, panX, panY };
}

/** live.getDiffusion's answer; no row yet is the cover alone. */
export function parseDiffusion(data: unknown): Diffusion {
  const answer = isRecord(data) ? data : {};
  return { base: base(answer.base), window: windowOf(answer.window), view: framing(answer.view) };
}

/** campaign.getCampaign's answer; null when it names no campaign. */
export function parseCampaign(data: unknown): Campaign | null {
  if (!isRecord(data) || text(data.name) === '') return null;
  const next = isRecord(data.nextSession) ? data.nextSession : null;
  const order = next === null ? null : num(next.order);
  return {
    id: text(data.id),
    name: text(data.name),
    cover: text(data.cover),
    art: text(data.art),
    glyph: text(data.glyph),
    session: next !== null && order !== null && text(next.title) !== '' ? { order, title: text(next.title) } : null,
  };
}

/** The ratio of an image: its pixels when the library gives them, else the ratio it states; a square if it says nothing. */
function ratioOf(entry: Record<string, unknown>): number {
  const [width, height] = [num(entry.width), num(entry.height)];
  if (width !== null && height !== null && width > 0 && height > 0) return width / height;
  const ratio = num(entry.ratio);
  return ratio !== null && ratio > 0 ? ratio : 1;
}

/** media.listMedia's answer, reduced to the illustrations. */
export function parseMedia(data: unknown): Media {
  const answer = isRecord(data) ? data : {};
  const illustrations: Illustration[] = [];
  if (Array.isArray(answer.illustrations)) {
    for (const entry of answer.illustrations as unknown[]) {
      if (isRecord(entry) && text(entry.id) !== '') {
        illustrations.push({ id: text(entry.id), title: text(entry.title), art: text(entry.art), glyph: text(entry.glyph), ratio: ratioOf(entry) });
      }
    }
  }
  return { cover: text(answer.cover), illustrations };
}

/** The props, the pilot tab being asked for by `pilot` or by `?pilote=1` in the address. */
export function parseProps(raw: Record<string, unknown>, search: string): Props {
  return { pilot: raw.pilot === true || new URLSearchParams(search).get('pilote') === '1' };
}

export const illustrationOf = (media: Media, id: string): Illustration | undefined => media.illustrations.find((i) => i.id === id);

const clamp = (value: number, low: number, high: number): number => Math.max(low, Math.min(high, value));

/** The window's height as a fraction of the stage's height: the image's ratio gives it (`window-proportional`), it is never stored. */
export const heightOf = (box: Box, ratio: number, stage: Stage): number => (box.w * stage.width) / ratio / stage.height;

/** The window dragged by its bar: it stays whole on the stage. */
export function moved(box: Box, dx: number, dy: number, ratio: number, stage: Stage): Box {
  const height = heightOf(box, ratio, stage);
  return { ...box, x: clamp(box.x + dx / stage.width, 0, Math.max(0, 1 - box.w)), y: clamp(box.y + dy / stage.height, 0, Math.max(0, 1 - height)) };
}

/** The widest the window may be: the stage's width from its left edge, and what keeps its image's height on the stage from its top. */
export const widest = (box: Box, ratio: number, stage: Stage): number => Math.max(MIN_WIDTH, Math.min(1 - box.x, ((1 - box.y) * stage.height * ratio) / stage.width));

/** The box a corner dragged by (dx, dy) draws, as fractions of the stage's width — what live.resizeWindow is asked for. */
export function dragged(box: Box, dx: number, dy: number, ratio: number, stage: Stage): { w: number; h: number } {
  const [w, h] = [box.w + dx / stage.width, box.w / ratio + dy / stage.width];
  const limit = widest(box, ratio, stage);
  return { w: clamp(w, MIN_WIDTH, limit), h: clamp(h, MIN_WIDTH / ratio, limit / ratio) };
}

/** The window the dragged box holds at the image's ratio: the largest that fits it, never distorted. */
export function resized(box: Box, drag: { w: number; h: number }, ratio: number, stage: Stage): Box {
  return { ...box, w: clamp(Math.min(drag.w, drag.h * ratio), MIN_WIDTH, widest(box, ratio, stage)) };
}

/** `POST /commands` for one DataCapability, by identifier, aimed at the diffusion. */
export function commandBody(dataCapability: string, payload: Record<string, unknown>): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: { aggregate: AGGREGATE }, payload, caller: { by: 'dm', from: 'PlayerStage', roles: [] } };
}

const VIOLATIONS: Record<string, string> = {
  'window-fields-together': 'aucune fenêtre n’est ouverte',
  'window-proportional': 'la fenêtre doit garder les proportions de l’image',
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
