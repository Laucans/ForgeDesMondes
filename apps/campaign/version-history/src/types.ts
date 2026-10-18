// The two sides of the Micro-UI: the shell it is handed, and the history
// `campaign.listEntityVersions` answers (EntityVersion@1).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
  /** Told each time the data moved; answers how to stop. The shell's client has it, a bare fake need not. */
  onDataVersion?(listener: (dataVersion: number) => void): () => void;
}

/** One field that moved since the previous version. */
export interface Change {
  field: string;
  old: string;
  new: string;
}

export interface Version {
  v: number;
  date: string;
  /** How the screen words who made it: `manuelle`, `ia`, `dndbeyond`. */
  origin: string;
  /** `dm`, `ia`, `dndbeyond`, `brief` or `restauration`; any other text is shown as it is. */
  source: string;
  /** The number of the version a restoration put back, else null. */
  restoredFrom: number | null;
  /** The DM was told of it already. Not part of EntityVersion@1: a Capability that says so is believed, else the version is seen. */
  seen: boolean;
  changes: Change[];
}

export interface History {
  name: string;
  /** Newest first. */
  versions: Version[];
}

/** The props of micro-ui.json. */
export interface Props {
  entity?: string;
  title?: string;
}
