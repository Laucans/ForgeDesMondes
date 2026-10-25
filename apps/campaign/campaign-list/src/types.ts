// The two sides of the Micro-UI: the shell it is handed, and the campaigns
// `campaign.listCampaigns` answers (crates/campaign/capabilities/list-campaigns).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

export interface CampaignCard {
  id: string;
  name: string;
  type: 'campagne' | 'oneshot';
  art: string;
  glyph: string;
  pitch: string;
  /** Counted from the campaign's sessions: 0 when it has none. */
  sessions: number;
  /** The day of the last session played; null before the first. */
  lastPlayed: string | null;
  /** The day of the next session; null when none is planned. */
  next: string | null;
}

/** The archive an export answers once applied: its file's name, and its content. */
export interface Archive {
  file: string;
  content: unknown;
}
