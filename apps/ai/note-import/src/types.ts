// The two sides of the Micro-UI: the shell it is handed, and what
// `campaign.listImportedNotes` answers.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

/** A note already imported: what became of it. */
export interface Import {
  id: string;
  /** ISO date, `YYYY-MM-DD`. */
  date: string;
  title: string;
  chars: number;
  proposed: number;
  created: number;
}

/** One element the AI proposes from a note, awaiting the DM's validation. */
export interface Proposal {
  id: string;
  type: string;
  name: string;
  excerpt: string;
  /** The id of the element the campaign already has under that name or alias, or null. */
  existing: string | null;
  checked: boolean;
}

export interface Notes {
  imports: Import[];
  proposals: Proposal[];
}

/** What the status line shows. */
export type Outcome = { kind: 'none' } | { kind: 'busy'; text: string } | { kind: 'ok' | 'error'; text: string };

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

/** Where dictated text comes from: the host that hears the microphone hands the transcript over. */
export interface Dictation {
  /** The text of a dictation that has just paused; '' when there is none. */
  transcript(): string;
}
