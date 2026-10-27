// The two sides of the Micro-UI: the shell it is handed, and what
// live.listSuggestions answers. Nothing else is read.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The data moved: told each time, polled; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void, everyMs?: number): () => void;
}

export interface Props {
  /** The campaign whose live session is listened to; empty when the screen names none and the server's live session decides. */
  campaign: string;
}

export type Kind = 'entity' | 'sound';

/** One thing heard at the table: an element of the campaign or a sound, and the words it was heard by. */
export interface Suggestion {
  id: string;
  kind: Kind;
  /** The element's type; empty for a sound. */
  type: string;
  name: string;
  /** The matched text. */
  via: string;
  /** An ISO instant; empty when the answer gives none. */
  detectedAt: string;
}

export interface Suggestions {
  /** The 5 most recent, newest first. */
  stack: Suggestion[];
  /** Every suggestion, by detection time descending. */
  history: Suggestion[];
}

/** How a command is followed until it settles, and how often the data is asked while the microphone is on. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
  watchMs: number;
  /** How long an element stays on top before the next one heard takes its place. */
  beatMs: number;
}

/** The microphone as the kit's MicButton draws it. */
export type MicState = 'idle' | 'recording' | 'paused';
