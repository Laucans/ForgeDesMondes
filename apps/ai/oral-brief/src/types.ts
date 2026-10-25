// The two sides of the Micro-UI: the shell it is handed, and what
// `session.getLastBrief` answers.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

/** One scene of the session, by order, with the names of the elements linked to it. */
export interface Scene {
  order: number;
  title: string;
  links: string[];
}

/** An element of the campaign the brief named. */
export interface Recognized {
  id: string;
  type: string;
  name: string;
  /** The words of the transcript that named it, or null when none did. */
  heard: string | null;
  /** True when the brief wrote a new version of it. */
  updated: boolean;
  change: string;
}

/** An element the campaign lacks: on the to-create list, only listed here. */
export interface Missing {
  id: string;
  type: string;
  name: string;
}

export interface Brief {
  /** ISO local date-time, or null while the brief has not been analysed. */
  lastRun: string | null;
  /** `mm:ss`, or null. */
  duration: string | null;
  /** The transcribed text, one line per entry. */
  transcript: string[];
  scenes: Scene[];
  recognized: Recognized[];
  missing: Missing[];
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
  /** The text heard since the last call — only the new part, never the whole dictation again; '' when there is none. */
  transcript(): string;
  /** Begins a take, when the host records one; a host that only hears words has nothing to start. */
  start?(): void;
  /**
   * The audio recorded since the last call, base64, or '' when there is none; ending the take in progress.
   * Only a dictation that gave no text is sent as audio (`dictation`): the port transcribes it, the audio goes no further.
   */
  audio(): string | Promise<string>;
}
