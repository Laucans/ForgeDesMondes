// The two sides of the Micro-UI: the shell it is handed, and the library
// `soundbox.listSounds` answers (mock/fixtures/soundbox.listSounds.json).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** Told each time the data moved; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void, everyMs?: number): () => void;
  /** The screen's context: the campaign is read from it. */
  context: {
    get(name: string): unknown;
    on(listener: (name: string, value: unknown) => void): () => void;
  };
}

export interface Sound {
  id: string;
  name: string;
  tags: string[];
  /** The file's length in seconds, when the library knows it. */
  duration: number | null;
  /** The sound's own volume, in percent. */
  volume: number;
  file: string;
  /** The effective volume the sound plays at, a fraction in [0, 1]: what the Capability computes (EffectiveVolume@1). */
  gain: number;
  /** A playback of the sound is running (no `stopped_at`). */
  playing: boolean;
}

export interface Library {
  /** The campaign's master volume, in percent. */
  masterVolume: number;
  sounds: Sound[];
}
