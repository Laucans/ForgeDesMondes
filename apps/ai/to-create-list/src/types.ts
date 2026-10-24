// The two sides of the Micro-UI: the shell it is handed, and what
// `campaign.listEntitiesToCreate` and `campaign.getEntity` answer.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** `GET /dataVersion`, polled: told each time the data moved; answers how to stop. */
  onDataVersion?(listener: (dataVersion: number) => void, everyMs?: number): () => void;
}

/** One element a brief left to create. */
export interface Item {
  id: string;
  /** `pnj`, `lieu` or `événement`. */
  type: string;
  name: string;
  session: string | null;
  sessionTitle: string;
  excerpt: string;
  /** `created` only when the element exists: to-create-leaves-once-created. */
  status: 'todo' | 'created';
  entity: string | null;
}

export interface Ability {
  key: string;
  score: number | null;
  modifier: number | null;
}

/** What the AI proposed, as the element it created holds it. */
export interface Proposal {
  name: string;
  description: string;
  aliases: string[];
  background: string;
  behavior: string;
  /** Present for an NPC with a sheet. */
  sheet: { ca: number | null; pvMax: number | null; abilities: Ability[]; features: Array<{ name: string; text: string }> } | null;
}

/** What the status line of an entry shows. */
export type Outcome = { kind: 'busy'; text: string } | { kind: 'error'; text: string };

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
