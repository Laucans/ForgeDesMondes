// The two sides of the Micro-UI: the shell it is handed, and what
// `campaign.getEntity` answers.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The screen's context: the sheet on show is `entity`, the campaign a new sheet joins is `campaign`. */
  context?: { get(name: string): unknown; set(name: string, value: unknown): void };
}

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

export type Kind = 'pj' | 'pnj' | 'lieu' | 'evenement';
export const ABILITIES = ['FOR', 'DEX', 'CON', 'INT', 'SAG', 'CHA'] as const;
export type Ability = (typeof ABILITIES)[number];

export interface Feature {
  name: string;
  text: string;
}

export interface Crop {
  zoom: number;
  x: number;
  y: number;
}

/** The sheet as the DM edits it. The modifiers are not here: they are derived from the scores (dex-modifier-derived). */
export interface Entity {
  /** '' for a sheet not saved yet. */
  id: string;
  kind: Kind;
  name: string;
  aliases: string[];
  description: string;
  notes: string;
  /** A PC's player, else ''. */
  player: string;
  prompt: string;
  background: string;
  behavior: string;
  stats: Record<Ability, number>;
  ca: number;
  pvMax: number;
  features: Feature[];
  /** The token's colour name (`accent`, `success`…). */
  color: string;
  /** The media chosen for the portrait or the token in this sitting: undefined = unchanged, null = removed. */
  portraitMedia: string | null | undefined;
  tokenMedia: string | null | undefined;
  /** What the read says the token has. */
  portrait: boolean;
  tokenImage: boolean;
  crop: Crop;
  ddb: { url: string; syncedAt: string | null } | null;
}

export interface Player {
  id: string;
  name: string;
}

export interface Media {
  id: string;
  title: string;
}

/** One field that moved. */
export interface Change {
  field: string;
  old: string;
  new: string;
}

/** What the status line shows. */
export type Outcome = { kind: 'none' } | { kind: 'busy'; text: string } | { kind: 'ok' | 'error'; text: string };

/** A request to open a sheet: an element of the campaign, or a new one of a kind. */
export type Target = { id: string } | { fresh: Kind };
