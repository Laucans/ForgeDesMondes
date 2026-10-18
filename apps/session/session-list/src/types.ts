// The two sides of the Micro-UI: the shell it is handed, and what
// `session.listSessions` and `campaign.listCharacters` answer.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

/** A session of the list, in the order of its `order` (the session number). */
export interface Session {
  id: string;
  order: number;
  title: string;
  /** The day it is planned or was played, `YYYY-MM-DD`. */
  date: string | null;
  /** `prep`, `live` or `closed` (SessionLifecycle@1); the mock-up's fixture also writes `played`. */
  status: string;
  /** The status in words, as SessionLifecycle@1 gives it. */
  label: string | null;
  /** The ids of the PCs present. */
  present: string[];
  summary: string | null;
}

export interface Character {
  id: string;
  name: string;
  /** `pj` for a player's character, `pnj` for the DM's. */
  kind: string;
}

/** What the creation dialog holds. */
export interface Form {
  title: string;
  plannedOn: string;
  present: string[];
}
