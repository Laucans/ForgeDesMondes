// The two sides of the Micro-UI: the shell it is handed, and the settings
// `ai.getSettings` answers (crates/ai/capabilities/get-settings).

/** The part of the shell's client (apps/shell/src/client/shell.ts) this Micro-UI uses. */
export interface Shell {
  /** `POST /capabilities/<id>`; no hash names the Capability alone, and the server holds its registered query. */
  capability(id: string, sha256: string | undefined, variables?: Record<string, unknown>): Promise<{ asOf: number; data: unknown }>;
  command(body: unknown): Promise<unknown>;
  commandResult(commandId: string): Promise<unknown>;
}

export type Mode = 'api' | 'binary';
export type Binary = 'opencode' | 'claude';

export interface Models {
  generationModel: string;
  summaryModel: string;
}

export interface ApiProvider extends Models {
  label: string;
  url: string;
  /** The key, masked: never the clear key. */
  key: string;
}

export interface LastTest {
  ok: boolean;
  at: string;
  provider: string | null;
  latency: string | null;
  message: string;
}

export interface Settings {
  mode: Mode;
  /** The active provider: an API type (`cloud`, `opencode`) or a binary (`opencode`, `claude-code`). */
  provider: string | null;
  binary: (Models & { binary: string }) | null;
  providers: Record<string, ApiProvider>;
  binaries: Record<string, Models & { label?: string }>;
  lastTest: LastTest | null;
  local: string[];
  sent: string[];
}

/** What the screen holds while the DM edits: nothing of it is saved until « Enregistrer ». */
export interface Form extends Models {
  mode: Mode;
  binary: Binary;
  url: string;
  key: string;
}

/** What the result area shows. */
export type Outcome =
  | { kind: 'none' }
  | { kind: 'last'; test: LastTest }
  | { kind: 'busy'; text: string }
  | { kind: 'ok' | 'error'; text: string };
