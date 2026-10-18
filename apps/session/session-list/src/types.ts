// The two sides of the Micro-UI: the shell it is handed, and what
// `session.listSessions` and `campaign.listCharacters` answer
// (crates/session/capabilities/list-sessions, crates/campaign/capabilities/list-characters).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

/** A session of the list. The status is the Capability's (`prep`, `live`, `closed`; the mock-up's older `played` is a closed one). */
export interface Session {
  id: string;
  order: number;
  title: string;
  date: string | null;
  status: string;
  /** The label SessionLifecycle@1 gives the status, when the Capability answers it. */
  label: string | null;
  /** The ids of the PCs present. */
  present: string[];
  summary: string | null;
}

/** The part of a character the list needs: who it is, and whether it is a PC. */
export interface Character {
  id: string;
  name: string;
  kind: string;
}

/** What the creation form holds while the DM edits it: nothing of it is sent until « Créer la séance ». */
export interface Form {
  title: string;
  date: string;
  present: string[];
}

/** What the open form says of the command it sent. */
export type Notice = { kind: 'none' } | { kind: 'busy' } | { kind: 'error'; text: string };

/** Where the reading of the sessions stands. */
export type Reading = 'loading' | 'ready' | 'failed';
