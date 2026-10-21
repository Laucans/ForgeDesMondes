// The two sides of the Micro-UI: the shell it is handed, and what the
// Capabilities answer (session.getSession, session.listScenes,
// campaign.listCharacters, media.listMedia, table.getBattlemapInstance).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The screen's context: the session being prepared is `session` when the screen sets it. */
  context?: { get(name: string): unknown };
}

export interface Session {
  id: string;
  title: string;
  order: number | null;
  /** The campaign the session belongs to, when the Capability says so. */
  campaign: string | null;
  /** The ids of the characters ticked present. */
  present: string[];
}

export interface Character {
  id: string;
  name: string;
  kind: 'pj' | 'pnj';
}

export interface Scene {
  id: string;
  title: string;
  places: string[];
  characters: string[];
  events: string[];
  media: string[];
  toCreate: string[];
  /** The id of the scene's battlemap instance, null while it has none. */
  battlemap: string | null;
  notes: string;
}

export interface Media {
  id: string;
  title: string;
}

export interface Medias {
  battlemaps: Media[];
  illustrations: Media[];
}

/** What the board shows of an instance: the map it was made from and how many tokens it holds. */
export interface Instance {
  id: string;
  battlemap: string;
  tokens: number;
  hidden: number;
}

/** Everything the board shows, as the five Capabilities answered it. */
export interface Board {
  session: Session;
  scenes: Scene[];
  characters: Character[];
  medias: Medias;
  /** By instance id. */
  instances: Record<string, Instance>;
}
