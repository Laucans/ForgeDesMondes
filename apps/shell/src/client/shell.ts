// The shell's client: what a Micro-UI calls the product with. Only the wire
// protocol — a Capability or a DataCapability by identifier, over the shell's
// own origin, which forwards to the server — never the code of either.

import { Context } from './context.js';

export interface Read {
  asOf: number;
  data: unknown;
}

export interface Shell {
  /** `POST /capabilities/<id>`: a Capability's answer to its persisted query (registered by `sha256`), as of a data version. */
  capability(id: string, sha256: string, variables?: Record<string, unknown>): Promise<Read>;
  /** `POST /commands`: submits a command; answers the server's body. */
  command(body: unknown): Promise<unknown>;
  /** `GET /commands/<id>`: where a command stands. */
  commandResult(commandId: string): Promise<unknown>;
  /** `GET /dataVersion`, polled: told each time the data moved; answers how to stop. */
  onDataVersion(listener: (dataVersion: number) => void, everyMs?: number): () => void;
  context: Context;
}

export class ProtocolError extends Error {
  constructor(
    readonly status: number,
    readonly body: unknown,
  ) {
    super(`the server answered ${status}`);
  }
}

async function call(fetcher: typeof fetch, method: string, path: string, body?: unknown): Promise<unknown> {
  const init: RequestInit = { method };
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const answer = await fetcher(path, init);
  const text = await answer.text();
  let parsed: unknown = text;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    // Not JSON (a gateway's page, the shell's own plain-text answer): the status still tells.
  }
  if (!answer.ok) throw new ProtocolError(answer.status, parsed);
  return parsed;
}

const POLL_MS = 2000;

export function createShell(fetcher: typeof fetch = (input, init) => fetch(input, init)): Shell {
  return {
    capability: async (id, sha256, variables = {}) => (await call(fetcher, 'POST', `/capabilities/${encodeURIComponent(id)}`, { sha256, variables })) as Read,
    command: (body) => call(fetcher, 'POST', '/commands', body),
    commandResult: (commandId) => call(fetcher, 'GET', `/commands/${encodeURIComponent(commandId)}`),
    onDataVersion(listener, everyMs = POLL_MS) {
      let last: number | undefined;
      let inFlight = false;
      const poll = async (): Promise<void> => {
        // One question at a time: answers cannot overtake each other.
        if (inFlight) return;
        inFlight = true;
        try {
          const { dataVersion } = (await call(fetcher, 'GET', '/dataVersion')) as { dataVersion: number };
          if (dataVersion !== last) {
            last = dataVersion;
            listener(dataVersion);
          }
        } catch {
          // The server is not there for now: the next poll tries again.
        } finally {
          inFlight = false;
        }
      };
      void poll();
      const timer = setInterval(() => void poll(), everyMs);
      return () => clearInterval(timer);
    },
    context: new Context(),
  };
}
