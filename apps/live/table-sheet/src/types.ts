// The two sides of the Micro-UI: the shell it is handed, and what the
// Capabilities answer (campaign.getEntity, media.listMedia).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

export type ElementType = 'character' | 'place' | 'event' | 'illustration';

/** The element the sheet opens: its type when the screen knows it, its id. */
export interface Target {
  type: ElementType | '';
  id: string;
}

export interface Props extends Target {
  /** The campaign whose player view a send reaches; empty when the screen names none. */
  campaign: string;
}

export const ABILITIES = ['FOR', 'DEX', 'CON', 'INT', 'SAG', 'CHA'] as const;
export type Ability = (typeof ABILITIES)[number];

export interface Feature {
  name: string;
  text: string;
}

/** A character met in a place: its id, and its name once it is read. */
export interface Ref {
  id: string;
  name: string;
  pc: boolean;
}

export interface Entity {
  id: string;
  name: string;
  aliases: string[];
  type: Exclude<ElementType, 'illustration'>;
  /** A player character, as opposed to a non-player one. */
  pc: boolean;
  description: string;
  notes: string;
  behavior: string;
  background: string;
  /** The stat block's subtitle: what the creature is. */
  creature: string;
  stats: Record<Ability, number | null>;
  modifiers: Record<Ability, number | null>;
  ca: number | null;
  pvMax: number | null;
  speed: string;
  features: Feature[];
  /** The palette name of the placeholder portrait, when the answer gives one. */
  art: string;
  initials: string;
  /** The id of the illustration the answer links to the element, when it does. */
  illustration: string;
  /** The ids of the characters met in a place, when the answer lists them. */
  characters: string[];
  /** The campaign the answer names, when it does. */
  campaign: string;
}

export interface Illustration {
  id: string;
  title: string;
  art: string;
  glyph: string;
  /** The place the library attaches the image to; empty for none. */
  place: string;
  file: string;
  size: string;
}

export interface Media {
  illustrations: Illustration[];
}
