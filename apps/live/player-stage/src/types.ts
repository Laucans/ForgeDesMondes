// The two sides of the Micro-UI: the shell it is handed, and what the
// Capabilities answer (live.getDiffusion, campaign.getCampaign,
// media.listMedia). Only what the players may see is ever parsed from them.

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** The data moved: told each time, polled; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void, everyMs?: number): () => void;
  /** The screen's context, when the shell has one: what a Micro-UI composed beside this one follows. */
  context?: { set(name: string, value: unknown): void };
}

export interface Props {
  /** The player view's tab the DM drives: the window is dragged and resized there. */
  pilot: boolean;
  /** The campaign whose player view this is; empty when the screen names none and the server's live session decides. */
  campaign: string;
}

export type BaseKind = 'cover' | 'illustration' | 'battlemap';

export interface Base {
  kind: BaseKind;
  id: string;
}

/**
 * The floating window as the row stores it: a position and a width, each a
 * fraction of the stage (x and w of its width, y of its height), so the
 * projected screen and the DM's tab draw the same window at any size. The
 * height is never stored: the image's ratio gives it.
 */
export interface Box {
  x: number;
  y: number;
  w: number;
}

export interface Window extends Box {
  media: string;
}

/** The battlemap's zoom and pan as the DM framed them. */
export interface Framing {
  zoom: number;
  panX: number;
  panY: number;
}

export interface Diffusion {
  base: Base;
  window: Window | null;
  view: Framing | null;
}

export interface Campaign {
  id: string;
  name: string;
  /** The illustration that is the campaign's cover. */
  cover: string;
  art: string;
  glyph: string;
  session: { order: number; title: string } | null;
}

export interface Illustration {
  id: string;
  title: string;
  art: string;
  glyph: string;
  /** Width over height of the image: 2/3 for 800 × 1200. */
  ratio: number;
}

export interface Media {
  cover: string;
  illustrations: Illustration[];
}

/** The stage's size in pixels. */
export interface Stage {
  width: number;
  height: number;
}
