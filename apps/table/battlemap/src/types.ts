// The two sides of the Micro-UI: the shell it is handed, and what the
// Capabilities answer (table.getBattlemapInstance, media.getBattlemap,
// campaign.listCharacters).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The data moved: told each time, polled; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void, everyMs?: number): () => void;
}

export type View = 'dm' | 'prep' | 'player';

export interface Props {
  instance: string;
  view: View;
  /** The player view's tab the DM drives: the tokens move there too. */
  pilot: boolean;
  fill: boolean;
  campaign: string;
}

/** The hex grid as HexGrid@1 defines it: `opacity` a fraction of 1, a hex always 5 feet. */
export interface Grid {
  size: number;
  ox: number;
  oy: number;
  orient: 'pointy' | 'flat';
  color: string;
  opacity: number;
}

export interface Shape {
  tag: string;
  attrs: Record<string, string>;
}

export interface MapInfo {
  id: string;
  title: string;
  width: number;
  height: number;
  grid: Grid;
  terrain: Shape[];
}

export interface Hex {
  q: number;
  r: number;
}

export interface Token extends Hex {
  id: string;
  char: string;
  kind: 'pj' | 'pnj';
  name: string;
  num: number | null;
  visible: boolean;
  /** The DM view only: the player view never carries a figure. */
  hp: number | null;
  hpMax: number | null;
  /** HealthBand@1's band, the only thing a player sees of an NPC's health. */
  healthBand: string | null;
  states: string[];
  dead: boolean;
  initials: string;
  color: string;
  portrait: boolean;
}

/** One line of the initiative order: the token, its initiative and the DEX that breaks a tie. */
export interface Participant {
  id: string;
  init: number;
  dex: number;
}

/** The combat under way on an instance: the round, whose turn it is, the order by rank. */
export interface Combat {
  round: number;
  current: string;
  order: Participant[];
}

export interface Instance {
  id: string;
  battlemap: string;
  /** The session whose players' view the map is sent to: the Diffusion aggregate's id. */
  session: string;
  label: string;
  roster: string[];
  tokens: Token[];
  combat: Combat | null;
  /** The initiative the DM entered for each PC of the scene, by character: kept to prefill the next combat. */
  pjInitiative: Record<string, number>;
}

export interface Character {
  id: string;
  name: string;
  tokenName: string;
  kind: 'pj' | 'pnj';
  initials: string;
  color: string;
  portrait: boolean;
  /** DEX modifier of the sheet: an NPC's initiative is its d20 plus this. */
  dexMod: number;
}

/** The pan and zoom of the map, as the DM frames it. */
export interface Frame {
  zoom: number;
  panX: number;
  panY: number;
}
