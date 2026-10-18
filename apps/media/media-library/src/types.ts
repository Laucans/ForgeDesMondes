// The two sides of the Micro-UI: the shell it is handed, and what the three
// Capabilities answer (fixtures media.listMedia, session.listScenes,
// campaign.listPlaces).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** Told each time the data moved (another screen's command included); answers how to stop. */
  onDataVersion?(listener: (dataVersion: number) => void): () => void;
  /** The screen's context: how « Régler la grille » tells GridSettings which battlemap to open. */
  context?: { set(name: string, value: unknown): void };
}

export type Kind = 'battlemap' | 'illustration';

export interface Medium {
  id: string;
  kind: Kind;
  title: string;
  file: string;
  size: string;
  /** The id of its campaign place, or nothing: a medium with no place stays valid. */
  place: string | null;
  art: string;
  glyph: string;
  /** A battlemap's grid, for its caption. */
  grid: { orient: string; size: number } | null;
  /** An illustration's width over height. */
  ratio: number | null;
}

export interface Place {
  id: string;
  name: string;
}

/** What the screen hands the Micro-UI. */
export interface Props {
  /** The session in preparation; empty when there is none. */
  session: string;
  /** Its number, as « Associé à la séance N » says it. */
  sessionNumber: number | null;
}

/** The upload form as the DM has filled it so far. */
export interface Draft {
  kind: Kind;
  title: string;
  place: string;
}

/** How a card stands against the session. */
export type Association =
  | { kind: 'none' }
  | { kind: 'in'; place: string }
  | { kind: 'out'; place: string };
