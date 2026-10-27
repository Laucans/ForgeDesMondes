// The two sides of the Micro-UI: the shell it is handed, and what its
// Capabilities answer (mock/fixtures/<identifier>.json).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  /** `POST /commands`: submits a command; answers the server's body. */
  command(body: unknown): Promise<unknown>;
  /** `GET /commands/<id>`: where a command stands. */
  commandResult(commandId: string): Promise<unknown>;
  /** Told each time the data moved; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void): () => void;
  /** The screen's context: the session, its campaign, the current scene and its battlemap are published there for the others. */
  context: { set(name: string, value: unknown): void };
}

export interface Scene {
  id: string;
  order: number;
  title: string;
  /** The id of the scene's battlemap instance; null when it has none. */
  battlemap: string | null;
}

export interface Live {
  id: string;
  /** The id of the session's campaign; empty when the answer names none. */
  campaign: string;
  order: number;
  title: string;
  /** `HH:MM`, the time the session started. */
  startedAt: string;
  /** Whether the microphone listens; null when the answer does not say. */
  micOn: boolean | null;
  /** Empty until `session.listScenes` has answered. */
  scenes: Scene[];
  currentScene: number;
}

/** What the players see: the base of their view, and the window floating over it. */
export interface Diffusion {
  base: { kind: 'cover' | 'illustration' | 'battlemap'; title: string };
  window: { title: string } | null;
}

export interface Recap {
  journalLines: number;
  duration: string;
  summaryModel: string;
  summary: string;
  mentions: string[];
  threads: string[];
}
