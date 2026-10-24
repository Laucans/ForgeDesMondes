// The two sides of the Micro-UI: the shell it is handed, and what the six
// Capabilities answer (the shapes of their capability.json, read tolerantly:
// a field the store does not hold yet is absent, never invented).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  /** Told each time the data moved; answers how to stop. The shell's client has it, a bare fake need not. */
  onDataVersion?(listener: (dataVersion: number) => void): () => void;
  /** The screen's context: the campaign shown is `campaign`, which the sheet beside the rail follows. */
  context?: { set(name: string, value: unknown): void };
}

/** What the sheet is asked to open: an element by its id, or a new one of a kind (`pnj`, `lieu`, `evenement`). */
export interface OpenRequest {
  id?: string;
  new?: string;
}

/** What the sheet answers once it shows an element (`id` is empty for one not saved yet). */
export interface Opened {
  id?: string | null;
}

/** The screen's words with the sheet beside the rail: this Micro-UI asks, the sheet opens. */
export interface Bus {
  /** The open request: this Micro-UI opens nothing itself. */
  open(request: OpenRequest): void;
  /** Leaves for another page of the screen (the tabs' links and « Nouveau… »). */
  navigate(href: string): void;
  /** Asks the sheet which element it shows. */
  query(): void;
  onOpened(listener: (opened: Opened) => void): () => void;
}

/** The props of micro-ui.json. */
export interface Props {
  mode?: string;
  campaign?: string;
}

export interface Player {
  id: string;
  name: string;
}

export interface Character {
  id: string;
  name: string;
  aliases: string[];
  kind: 'pj' | 'pnj';
  player: string | null;
  initials: string;
  /** « Elfe des bois, rôdeuse niveau 5 », when the store holds it. */
  type: string;
  art: string;
  ddbSyncedAt: string | null;
  hasDdb: boolean;
}

/** A place or an event. */
export interface Named {
  id: string;
  name: string;
  aliases: string[];
  art: string;
}

export interface Media {
  id: string;
  title: string;
  /** `Battlemap` or `Illustration`. */
  kind: 'battlemap' | 'illustration';
  size: string;
  art: string;
  glyph: string;
}

export interface Sound {
  id: string;
  name: string;
  tags: string[];
}

export type Group = 'pj' | 'pnj' | 'place' | 'event';

/** One line of the rail, whatever it comes from. */
export interface RailEntry {
  id: string;
  group: Group;
  name: string;
  aliases: string[];
  sub: string;
  art: string;
  glyph: string;
}
