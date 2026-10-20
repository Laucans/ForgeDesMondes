// What the panel knows, with no DOM: the campaign as the Capability answers
// it, the words it shows, the command the export button becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { Campaign, NextSession } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');

function nextSession(value: unknown): NextSession | null {
  if (!isRecord(value) || typeof value.order !== 'number' || typeof value.date !== 'string') return null;
  return { order: value.order, date: value.date };
}

/** The Capability's answer, or null for an unknown campaign (it answers `null`, not an error). */
export function parseCampaign(data: unknown): Campaign | null {
  if (!isRecord(data) || typeof data.id !== 'string' || typeof data.name !== 'string') return null;
  const counts: Record<string, number> = {};
  if (isRecord(data.counts)) for (const [name, value] of Object.entries(data.counts)) if (typeof value === 'number') counts[name] = value;
  return {
    id: data.id,
    name: data.name,
    type: data.type === 'oneshot' ? 'oneshot' : 'campagne',
    art: text(data.art),
    glyph: text(data.glyph),
    pitch: text(data.pitch),
    lastExport: typeof data.lastExport === 'string' && data.lastExport !== '' ? data.lastExport : null,
    counts,
    nextSession: nextSession(data.nextSession),
  };
}

/** The counts the panel can name, in the order the mock-up draws them. */
const COUNT_LABELS: ReadonlyArray<readonly [string, string]> = [
  ['sessions', 'séances'],
  ['players', 'joueurs'],
  ['characters', 'personnages'],
  ['places', 'lieux'],
  ['events', 'événements'],
  ['media', 'médias'],
  ['sounds', 'sons'],
];

/** Each count the answer carries, with its label: a count the Capability does not (yet) answer is not drawn. */
export function countsOf(campaign: Campaign): Array<{ value: number; label: string }> {
  const out: Array<{ value: number; label: string }> = [];
  for (const [name, label] of COUNT_LABELS) {
    const value = campaign.counts[name];
    if (value !== undefined) out.push({ value, label });
  }
  return out;
}

/** A day (`2026-10-04`) or a timestamp, in words; the day as it is written when it cannot be read. */
function parse(value: string): Date | null {
  const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T12:00:00` : value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function nextSessionDay(value: string): string {
  const date = parse(value);
  return date === null ? value : date.toLocaleDateString('fr-FR', { weekday: 'long', day: 'numeric', month: 'long' });
}

/** « Dernier export : 4 octobre 2026 », or that there is none yet. */
export function lastExportText(lastExport: string | null): string {
  if (lastExport === null) return 'Jamais exporté';
  const date = parse(lastExport);
  return `Dernier export : ${date === null ? lastExport : date.toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' })}`;
}

/** The id of the campaign to open: the prop, else `?id=` of the address. */
export function campaignId(props: Record<string, unknown>, search: string): string {
  return text(props.campaign).trim() || (new URLSearchParams(search).get('id') ?? '').trim();
}

/** `POST /commands` for the export, by identifier: the campaign is named by its id, nothing else is sent. */
export function exportCommand(dataCapability: string, id: string): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: { id }, caller: { by: 'dm', from: 'CampaignOverview', roles: [] } };
}

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
