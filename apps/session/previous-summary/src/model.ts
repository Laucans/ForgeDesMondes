// What the Capabilities answered, read defensively: a field that is missing
// or of the wrong type is an empty one, never an exception on the DM's
// screen. An answer with no summary is « no closed session yet », not an error.

import type { Journal, JournalLine, Mention, PreviousSession, Summary } from './types.ts';

type Dict = Record<string, unknown>;

const isDict = (value: unknown): value is Dict => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

function parseSession(value: unknown): PreviousSession | null {
  if (!isDict(value)) return null;
  const id = text(value.id);
  if (id === '') return null;
  return { id, order: typeof value.order === 'number' ? value.order : 0, title: text(value.title), date: text(value.date) };
}

function parseMention(value: unknown): Mention | null {
  if (!isDict(value)) return null;
  const id = text(value.id);
  return id === '' ? null : { id, type: text(value.type), name: text(value.name) };
}

/** The previous closed session and its summary (empty when none was written); null when there is no closed session yet. */
export function parseSummary(data: unknown): Summary | null {
  if (!isDict(data)) return null;
  const session = parseSession(data.session);
  if (session === null) return null;
  return {
    session,
    summary: text(data.summary),
    summaryMeta: text(data.summaryMeta),
    mentions: list(data.mentions).flatMap((m) => parseMention(m) ?? []),
    threads: list(data.threads).filter((t): t is string => typeof t === 'string'),
  };
}

function parseLine(value: unknown): JournalLine | null {
  if (!isDict(value)) return null;
  const line = { t: text(value.t), text: text(value.text) };
  return line.text === '' ? null : line;
}

/** The journal's lines in the order they were spoken. */
export function parseJournal(data: unknown): Journal {
  const answer = isDict(data) ? data : {};
  return { journal: list(answer.journal).flatMap((l) => parseLine(l) ?? []), journalStats: text(answer.journalStats) };
}

/** « Séance 3 — Le marché noir ». */
export const heading = (summary: Summary): string => `Séance ${summary.session.order} — ${summary.session.title}`;

/** The date in full, then where the summary came from. */
export function meta(summary: Summary): string {
  const date = new Date(`${summary.session.date}T12:00:00`);
  const day = Number.isNaN(date.getTime()) ? summary.session.date : date.toLocaleDateString('fr-FR', { dateStyle: 'full' });
  return [day, summary.summaryMeta].filter((part) => part !== '').join(' · ');
}

/** The kit's chip colours: a character, a place, anything else is a medium. */
export const chipKind = (type: string): 'pnj' | 'lieu' | 'media' => (type === 'pnj' ? 'pnj' : type === 'lieu' ? 'lieu' : 'media');
