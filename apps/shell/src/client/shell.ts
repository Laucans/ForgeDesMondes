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

/** How a command of this page is followed while a read waits for it: every `everyMs`, `polls` times at most. */
export interface Settling {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  polls: number;
}

const SETTLING: Settling = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 100, polls: 50 };

/** The states the single applier moves on its own: a read waits them out; one waiting for a person is never waited for. */
const ADVANCING = new Set(['queued', 'confirmed']);

const record = (value: unknown): Record<string, unknown> => (value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {});
/** Where a command stands, from `POST /commands`' answer or `GET /commands/<id>`'s. */
const stateOf = (answer: unknown): { id: string; status: string } => {
  const result = record(record(answer).result);
  const id = record(answer).commandId ?? result.commandId;
  return { id: typeof id === 'string' ? id : '', status: typeof result.status === 'string' ? result.status : '' };
};

/** The caller of a command (contract S): the surface of the screen it is sent from. */
export interface Caller {
  by: string;
  roles: string[];
  permissions: string[];
}

/** Who acts when anything is in doubt: the players', with no role and no permission. */
const FAIL_CLOSED: Caller = { by: 'player', roles: [], permissions: [] };

/** What the shell reads of its page: the address, the metas the shell's server wrote, and the page's own fetch. */
export interface Page {
  href: string;
  meta(name: string): string | undefined;
  fetch: typeof fetch;
}

const browserPage = (): Page => ({
  href: typeof location === 'undefined' ? '' : location.href,
  meta: (name) => (typeof document === 'undefined' ? undefined : (document.querySelector(`meta[name="${name}"]`)?.getAttribute('content') ?? undefined)),
  fetch: (input, init) => fetch(input, init),
});

const asStrings = (value: unknown): string[] | undefined =>
  Array.isArray(value) && value.every((item): item is string => typeof item === 'string') ? value : undefined;

/** A caller as a surface declares it (contract S); `undefined` when it is not whole. */
function asCaller(value: unknown): Caller | undefined {
  const caller = (value && typeof value === 'object' ? value : {}) as Record<string, unknown>;
  const roles = asStrings(caller.roles);
  const permissions = caller.permissions === undefined ? [] : asStrings(caller.permissions);
  return typeof caller.by === 'string' && caller.by && roles && permissions ? { by: caller.by, roles, permissions } : undefined;
}

async function json(page: Page, url: string): Promise<unknown> {
  const answer = await page.fetch(url);
  if (!answer.ok) throw new Error(`${url}: ${answer.status}`);
  return answer.json() as Promise<unknown>;
}

/** A file of the product the mock-up does not hold: `../__product/` beside its pages' folder, else the repository's root two folders up. */
async function product(page: Page, path: string): Promise<unknown> {
  try {
    return await json(page, new URL(`../__product/${path}`, page.href).href);
  } catch {
    return json(page, new URL(`../../${path}`, page.href).href);
  }
}

/** In the mock-up, where no shell's server wrote the metas: the caller its page's screen declares (`../pages.json`, `surfaces.json`, `apps/screens/<screen>.json`). */
async function mockupCaller(page: Page): Promise<Caller> {
  const here = new URL(page.href);
  const pagesUrl = new URL('../pages.json', here);
  const listed = (await json(page, pagesUrl.href)) as { pages?: Array<{ path?: string; screen?: string; surface?: string }> };
  const mocked = listed.pages?.find((p) => typeof p.path === 'string' && new URL(p.path, pagesUrl).pathname === here.pathname);
  if (!mocked) return FAIL_CLOSED;
  let surface = mocked.surface;
  if (mocked.screen !== undefined) {
    try {
      const screen = (await product(page, `apps/screens/${mocked.screen}.json`)) as { surface?: string; pilot?: { surface?: string; query?: string } };
      const pilot = screen.pilot;
      surface = pilot && typeof pilot.query === 'string' && here.searchParams.get(pilot.query) === '1' ? pilot.surface : screen.surface;
    } catch (error) {
      // A screen file that cannot be fetched: the page's own surface still says who acts; none, nothing does.
      if (surface === undefined) throw error;
    }
  }
  const declared = ((await product(page, 'surfaces.json')) as { surfaces?: Record<string, unknown> }).surfaces;
  return (surface !== undefined && declared && Object.hasOwn(declared, surface) ? asCaller(declared[surface]) : undefined) ?? FAIL_CLOSED;
}

/**
 * The caller every command of this page carries: the metas' when the shell's server wrote them, else the mock-up's;
 * the players' for any doubt. `settled` is false when a file could not be read: the next command asks again.
 */
async function pageCaller(page: Page): Promise<{ caller: Caller; settled: boolean }> {
  try {
    const meta = page.meta('shell-caller');
    if (meta !== undefined) return { caller: asCaller(JSON.parse(meta)) ?? FAIL_CLOSED, settled: true };
    return { caller: await mockupCaller(page), settled: true };
  } catch {
    return { caller: FAIL_CLOSED, settled: false };
  }
}

/** The body with the page's caller set over what the Micro-UI sent; only its `from` stays. */
function withCaller(body: unknown, caller: Caller): unknown {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return body;
  const sent = (body as { caller?: { from?: unknown } | null }).caller;
  const from = sent && typeof sent === 'object' && !Array.isArray(sent) ? sent.from : undefined;
  return { ...body, caller: { ...(from === undefined ? {} : { from }), by: caller.by, roles: caller.roles, permissions: caller.permissions } };
}

export function createShell(fetcher: typeof fetch = (input, init) => fetch(input, init), page: Page = browserPage(), settling: Settling = SETTLING): Shell {
  // Read-your-writes: a Capability read waits for the commands this page sent before it to leave the queue,
  // so a panel opened right after a save shows that save (the applier runs on its own clock).
  const writing = new Set<Promise<void>>();
  const follow = async (sent: Promise<unknown>): Promise<void> => {
    try {
      const first = stateOf(await sent);
      const { id } = first;
      let { status } = first;
      for (let asked = 0; ADVANCING.has(status) && id !== '' && asked < settling.polls; asked += 1) {
        await settling.sleep(settling.everyMs);
        ({ status } = stateOf(await call(fetcher, 'GET', `/commands/${encodeURIComponent(id)}`)));
      }
    } catch {
      // Refused or unreachable: nothing of it to wait for.
    }
  };
  const track = (sent: Promise<unknown>): void => {
    const settled = follow(sent).finally(() => writing.delete(settled));
    writing.add(settled);
  };
  let resolving: ReturnType<typeof pageCaller> | undefined;
  const callerOf = async (): Promise<Caller> => {
    const asked = (resolving ??= pageCaller(page));
    const { caller, settled } = await asked;
    // A failure to read is not remembered: a later command tries again, failing closed meanwhile.
    if (!settled && resolving === asked) resolving = undefined;
    return caller;
  };
  const context = new Context();
  // The context's campaign follows one precedence: the address, then the live session (a Micro-UI's write), then the default.
  const named = page.href ? new URL(page.href, 'http://shell').searchParams.get('campaign') : null;
  if (named) context.hold('campaign', named);
  else {
    const fallback = page.meta('shell-default-campaign');
    if (fallback) context.set('campaign', fallback);
  }
  return {
    capability: async (id, sha256, variables = {}) => {
      await Promise.all([...writing]);
      return (await call(fetcher, 'POST', `/capabilities/${encodeURIComponent(id)}`, { sha256, variables })) as Read;
    },
    command: (body) => {
      // Tracked before its first await: a read asked right after it, even before it is sent, waits for it.
      const sent = (async () => call(fetcher, 'POST', '/commands', withCaller(body, await callerOf())))();
      track(sent);
      return sent;
    },
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
    context,
  };
}
