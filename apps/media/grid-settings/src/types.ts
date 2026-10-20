// The two sides of the Micro-UI: the shell it is handed, and the battlemap
// `media.getBattlemap` answers (crates/media/capabilities/get-battlemap).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

export type Orient = 'pointy' | 'flat';

/** A grid as HexGrid@1 states it: the opacity is a fraction of 1. */
export interface Grid {
  size: number;
  ox: number;
  oy: number;
  orient: Orient;
  color: string;
  opacity: number;
}

/** One drawn shape of the map (`<tag class="…" …>`): the terrain, when the Capability answers any. */
export interface Shape {
  tag: string;
  attrs: Record<string, string>;
}

export interface Battlemap {
  id: string;
  title: string;
  width: number;
  height: number;
  grid: Grid;
  terrain: Shape[];
}

/** What the screen holds while the DM edits: the grid in the screen's units, the opacity a percentage. Nothing of it is saved until « Enregistrer ». */
export interface Form {
  size: number;
  ox: number;
  oy: number;
  orient: Orient;
  color: string;
  /** 0 to 100, as the slider shows it; the command carries it divided by 100. */
  opacity: number;
}

/** What the result area shows. */
export type Outcome = { kind: 'none' } | { kind: 'busy' | 'ok' | 'error'; text: string };

/** The cell of a hex grid, axial coordinates. */
export interface Hex {
  q: number;
  r: number;
}
