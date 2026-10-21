// What the Capabilities answered, read defensively: a field that is missing
// or of the wrong type is an empty one, never an exception on the DM's
// screen. And the commands this Micro-UI sends, and how their answers read.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Diffusion, Live, Recap, Scene } from './types.ts';

type Dict = Record<string, unknown>;

const isDict = (value: unknown): value is Dict => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const texts = (value: unknown): string[] => list(value).filter((t): t is string => typeof t === 'string');

function parseScene(value: unknown): Scene | null {
  if (!isDict(value)) return null;
  const id = text(value.id);
  return id === '' ? null : { id, order: typeof value.order === 'number' ? value.order : 0, title: text(value.title) };
}

/**
 * The running session. The header may sit at the root of the answer or under
 * `session`; its scenes, when the answer names them, beside or inside it.
 * Null when there is no running session.
 */
export function parseLive(data: unknown): Live | null {
  if (!isDict(data)) return null;
  const header = isDict(data.session) ? data.session : data;
  const id = text(header.id);
  if (id === '') return null;
  const scenes = list(header.scenes ?? data.scenes).flatMap((s) => parseScene(s) ?? []);
  const current = header.currentScene ?? data.currentScene;
  const mic = isDict(data.microphone) ? data.microphone.on : undefined;
  return {
    id,
    order: typeof header.order === 'number' ? header.order : 0,
    title: text(header.title),
    startedAt: text(header.startedAt),
    micOn: typeof mic === 'boolean' ? mic : null,
    scenes,
    currentScene: typeof current === 'number' && current >= 0 && current < scenes.length ? current : 0,
  };
}

const KINDS = new Set(['cover', 'illustration', 'battlemap']);

/** What the players see; the campaign cover while the answer says nothing else. */
export function parseDiffusion(data: unknown): Diffusion {
  const answer = isDict(data) ? data : {};
  const base = isDict(answer.base) ? answer.base : {};
  const kind = KINDS.has(text(base.kind)) ? (text(base.kind) as Diffusion['base']['kind']) : 'cover';
  const window = isDict(answer.window) ? answer.window : null;
  return { base: { kind, title: text(base.title) }, window: window === null ? null : { title: text(window.title) } };
}

/** « Les joueurs voient la couverture de la campagne » and the like, split so the subject can be set apart. */
export function showing(diffusion: Diffusion): { base: string; window: string | null } {
  const { kind, title } = diffusion.base;
  const base =
    kind === 'illustration'
      ? title === '' ? 'une illustration (scène)' : `« ${title} » (scène)`
      : kind === 'battlemap'
        ? title === '' ? 'une battlemap' : `la battlemap « ${title} »`
        : 'la couverture de la campagne';
  const window = diffusion.window === null ? null : diffusion.window.title === '' ? 'une fenêtre' : `fenêtre « ${diffusion.window.title} »`;
  return { base, window };
}

const MINUTES_PER_DAY = 24 * 60;

/** « 3 h 27 min », « 27 min »: from `HH:MM` to `now`, over midnight when need be. Empty when the start is not a time. */
export function durationSince(startedAt: string, now: Date): string {
  const match = /^(\d{1,2}):(\d{2})$/.exec(startedAt.trim());
  if (match === null) return '';
  const start = Number(match[1]) * 60 + Number(match[2]);
  const minutes = (((now.getHours() * 60 + now.getMinutes() - start) % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  const hours = Math.floor(minutes / 60);
  return hours === 0 ? `${minutes} min` : `${hours} h ${String(minutes % 60).padStart(2, '0')} min`;
}

/** The model that wrote the summary: `{ model }`, or the sentence « Généré à la clôture par <model>, retouché le… » cut at the model. */
function modelOf(meta: unknown): string {
  if (isDict(meta)) return text(meta.model);
  const sentence = text(meta);
  return /\bpar ([^,]+)/.exec(sentence)?.[1]?.trim() ?? sentence;
}

/** The closing recap, from the journal and the summary read once the session is closed. */
export function parseRecap(journal: unknown, summary: unknown, duration: string): Recap {
  const lines = isDict(journal) ? list(journal.journal) : [];
  const answer = isDict(summary) ? summary : {};
  return {
    journalLines: lines.length,
    duration,
    summaryModel: modelOf(answer.summaryMeta),
    summary: text(answer.summary),
    mentions: list(answer.mentions).flatMap((m) => (isDict(m) && text(m.name) !== '' ? [text(m.name)] : [])),
    threads: texts(answer.threads),
  };
}

/** « 2 114 lignes » — the French thousands separator is a no-break space. */
export const linesLabel = (count: number): string => `${count.toLocaleString('fr-FR')} ligne${count > 1 ? 's' : ''}`;

/**
 * `POST /commands` for one DataCapability, by identifier; `target` names the aggregate it acts on. The key names
 * one attempt: a retry after a refusal is a new attempt, and must not be answered with the first one's result.
 */
export function commandBody(dataCapability: string, idempotencyKey: string, target?: { aggregate: string; id: string }): unknown {
  return {
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload: {},
    ...(target === undefined ? {} : { target }),
    idempotencyKey,
    caller: { by: 'dm', from: 'LiveSessionBar', roles: [] },
  };
}

const VIOLATIONS: Record<string, string> = {
  'status-lifecycle': 'cette séance est déjà close',
  'close-session': 'la clôture a été refusée',
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
  const result = isDict(answer) && isDict(answer.result) ? answer.result : {};
  return { status: text(result.status), violations: texts(result.violations) };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isDict(answer) ? text(answer.commandId) || (isDict(answer.result) ? text(answer.result.commandId) : '') : '';
}
