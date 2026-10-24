// The two sides of the Micro-UI: the shell it is handed, and what the
// Capabilities answer (session.listScenes, campaign.listCharacters,
// campaign.listPlaces, campaign.listEvents, media.listMedia).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The screen's context, when the shell has one: the session and the campaign the panel reads for. */
  context?: { get(name: string): unknown; on?(listener: (name: string, value: unknown) => void): () => void };
}

export type ItemType = 'character' | 'place' | 'event' | 'battlemap' | 'illustration';

/** One element of the panel: what the DM opens, finds or sends. */
export interface Item {
  type: ItemType;
  id: string;
  name: string;
  aliases: string[];
  /** A character: PC or NPC. */
  kind?: 'pj' | 'pnj';
  /** A battlemap: the instance the session holds of it, when it holds one. */
  instance?: string;
}

export interface Scene {
  id: string;
  title: string;
  places: string[];
  characters: string[];
  events: string[];
  /** The ids of the media the scene links, battlemaps and illustrations alike. */
  media: string[];
  /** The id of the scene's battlemap instance. */
  battlemap: string | null;
}

export interface Medium {
  id: string;
  title: string;
}

export interface Library {
  battlemaps: Medium[];
  illustrations: Medium[];
}

/** What the five reads answered, parsed. */
export interface World {
  scenes: Scene[];
  characters: Item[];
  places: Item[];
  events: Item[];
  library: Library;
}

/** A labelled run of items: a scene's places, the campaign's NPCs. */
export interface Group {
  key: string;
  label: string;
  items: Item[];
}
