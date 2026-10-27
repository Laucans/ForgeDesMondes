// The two sides of the Micro-UI: the shell it is handed, and the sounds
// `soundbox.listSounds` answers (mock/fixtures/soundbox.listSounds.json).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** Told each time the data moved; answers how to stop. Optional: a shell that cannot tell leaves the list to its own refreshes. */
  onDataVersion?(listener: (dataVersion: number) => void): () => void;
  /** The screen's context: the campaign is read from it, and published to it once found. */
  context: {
    get(name: string): unknown;
    set(name: string, value: unknown): void;
  };
}

export interface Sound {
  id: string;
  name: string;
  tags: string[];
  /** The sound's own volume, in percent. */
  volume: number;
  file: string;
  /** In seconds; null when the Capability does not know it. */
  duration: number | null;
  /** Whether a playback of it runs now (the DM view's business: it only marks the row). */
  playing: boolean;
}

/** What the DM types to add a sound: nothing of it is sent until « Ajouter à la soundbox ». */
export interface AddForm {
  file: string;
  name: string;
  tags: string;
}

/** What the DM types to edit a sound. */
export interface EditForm {
  id: string;
  name: string;
  tags: string;
  volume: number;
}

/** What the card tells after a command. */
export type Outcome = { kind: 'none' } | { kind: 'busy' | 'ok' | 'error'; text: string };
