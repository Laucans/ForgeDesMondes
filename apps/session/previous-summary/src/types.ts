// The two sides of the Micro-UI: the shell it is handed, and what
// `session.getPreviousSessionSummary` and `session.getSessionJournal` answer
// (mock/fixtures/<identifier>.json).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses: it reads, and only reads. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
}

export interface PreviousSession {
  id: string;
  order: number;
  title: string;
  /** `YYYY-MM-DD`. */
  date: string;
}

export interface Mention {
  id: string;
  /** `pnj`, `lieu`, `pj`, `événement`…: the kit's chip colours the first three. */
  type: string;
  name: string;
}

export interface Summary {
  session: PreviousSession;
  summary: string;
  /** Where the summary came from: the model that wrote it, when it was edited. */
  summaryMeta: string;
  mentions: Mention[];
  threads: string[];
}

export interface JournalLine {
  /** The time the line was spoken, `HH:MM`. */
  t: string;
  text: string;
}

export interface Journal {
  journal: JournalLine[];
  journalStats: string;
}
