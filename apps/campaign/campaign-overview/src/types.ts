// The two sides of the Micro-UI: the shell it is handed, and the campaign
// `campaign.getCampaign` answers (crates/campaign/capabilities/get-campaign).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

/** The next session, once the session table answers it: absent from the answer until then. */
export interface NextSession {
  order: number;
  date: string;
}

export interface Campaign {
  id: string;
  name: string;
  type: 'campagne' | 'oneshot';
  art: string;
  glyph: string;
  pitch: string;
  /** The day of the last export; null before the first. */
  lastExport: string | null;
  /** Each count the answer carries, by name: the overview shows those it knows. */
  counts: Record<string, number>;
  nextSession: NextSession | null;
}
