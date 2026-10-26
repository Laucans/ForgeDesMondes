// What the Micro-UI knows, with no DOM: the campaigns as the Capability
// answers them, the words it shows, the commands its buttons become.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Archive, CampaignCard } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const day = (value: unknown): string | null => (typeof value === 'string' && value !== '' ? value : null);

/** The Capability's answer: an array of campaigns, newest first; an item with no id or name is not one. */
export function parseCampaigns(data: unknown): CampaignCard[] {
  const items = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.campaigns) ? data.campaigns : [];
  const out: CampaignCard[] = [];
  for (const item of items as unknown[]) {
    if (!isRecord(item) || typeof item.id !== 'string' || item.id === '' || typeof item.name !== 'string') continue;
    out.push({
      id: item.id,
      name: item.name,
      type: item.type === 'oneshot' ? 'oneshot' : 'campagne',
      art: text(item.art),
      glyph: text(item.glyph),
      pitch: text(item.pitch),
      sessions: typeof item.sessions === 'number' ? item.sessions : 0,
      lastPlayed: day(item.lastPlayed),
      next: day(item.next),
    });
  }
  return out;
}

/** A day (`2026-10-14`) or a timestamp, in words; the day as it is written when it cannot be read. */
export function dayText(value: string): string {
  const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T12:00:00` : value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' });
}

/** « 4 séances », « 0 séance » for a campaign with none. */
export const sessionsText = (sessions: number): string => `${sessions} séance${sessions > 1 ? 's' : ''}`;

export const lastPlayedText = (lastPlayed: string | null): string => (lastPlayed === null ? 'jamais jouée' : `jouée le ${dayText(lastPlayed)}`);

/** The glyph of a new cover: the first letter of its name. */
export const glyphOf = (name: string): string => [...name.trim()][0]?.toUpperCase() ?? '';

const caller = { by: 'dm', from: 'CampaignList', roles: [] };
const command = (dataCapability: string, payload: Record<string, unknown>, idempotencyKey?: string): unknown => ({
  dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
  payload,
  ...(idempotencyKey === undefined ? {} : { idempotencyKey }),
  caller,
});

export interface NewCampaign {
  name: string;
  type: 'campagne' | 'oneshot';
  pitch: string;
}

/** `POST /commands` for a new campaign; a blank pitch is none, the cover's glyph is the name's first letter. */
export function createCommand(dataCapability: string, form: NewCampaign, idempotencyKey: string): unknown {
  const name = form.name.trim();
  const pitch = form.pitch.trim();
  return command(dataCapability, { name, type: form.type, pitch: pitch === '' ? null : pitch, glyph: glyphOf(name) }, idempotencyKey);
}

/** `POST /commands` for the export: the campaign is named by its id, nothing else is sent. */
export const exportCommand = (dataCapability: string, id: string): unknown => command(dataCapability, { id });

/** `POST /commands` for the import: the archive as the export wrote it, nothing else. An import never names a campaign to overwrite. */
export const importCommand = (dataCapability: string, archive: unknown, idempotencyKey: string): unknown => command(dataCapability, { archive }, idempotencyKey);

/** Whether a command not applied may still be queued: one that timed out, or a server that could not be told why. Sent again, it must be the same command. */
export const mayBeQueued = (state: Settled): boolean => !isFinal(state.status) && !(state.status === 'failed' && state.violations.length > 0);

/** What a dropped file holds, if it is an archive an export wrote: a JSON document. */
export function parseArchive(source: string): unknown {
  try {
    const document: unknown = JSON.parse(source);
    return isRecord(document) ? document : undefined;
  } catch {
    return undefined;
  }
}

const FINAL = new Set(['applied', 'rejected', 'cancelled', 'expired']);

export interface Settled {
  status: string;
  violations: string[];
  /** What an applied export answers: the archive. */
  archive: Archive | null;
}

function archiveOf(value: unknown): Archive | null {
  if (!isRecord(value) || typeof value.file !== 'string' || value.file === '' || value.content === undefined || value.content === null) return null;
  return { file: value.file, content: value.content };
}

/** `{ result: { status, violations, attachment } }`, as the server answers a command and its lookup. */
export function settled(answer: unknown): Settled {
  const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
  const violations = Array.isArray(result.violations) ? result.violations.filter((v): v is string => typeof v === 'string') : [];
  return { status: text(result.status), violations, archive: archiveOf(result.attachment) };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}

/** A command the server could not be asked, or refused before queueing it (`why`, when it said). */
export const failed = (why: string): Settled => ({ status: 'failed', violations: why === '' ? [] : [why], archive: null });

/** How an operation names what went wrong when its command was not applied: the refusal, the wait that ran out, the failure. */
export interface Words {
  refused: string;
  pending: string;
  failed: string;
}

/** Why a command was not applied, in words. */
export function unapplied(state: Settled, words: Words): string {
  const reasons = state.violations.length > 0 ? state.violations.join(', ') : 'raison inconnue';
  if (state.status === 'failed' && state.violations.length === 0) return words.failed;
  if (state.status === 'rejected' || state.status === 'failed') return `${words.refused} : ${reasons}.`;
  return `${words.pending} : état ${state.status || 'inconnu'}.`;
}

/** The file name a download is saved under: the one the export gave, with nothing that is a path. */
export const safeFileName = (file: string): string => file.replace(/[\\/:*?"<>|]+/g, '-');

/** Why the server refused a command before queueing it (`422 { error }`), in the words it gave; none for any other failure. */
export function refusedBy(error: unknown): string {
  const body = isRecord(error) ? error.body : undefined;
  return isRecord(body) ? text(body.error) : '';
}
