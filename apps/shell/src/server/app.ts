// The shell's HTTP surface: the home, a frozen screen at `/screens/<screen>`,
// the files its pages load, the sound library's files at `/sounds/<file>`
// (mock/sounds/), and the wire protocol forwarded to the server.

import { createReadStream } from 'node:fs';
import { readdir, readFile, stat } from 'node:fs/promises';
import type { IncomingMessage, RequestListener, ServerResponse } from 'node:http';
import { join, resolve, sep } from 'node:path';
import type { Config } from './config.js';
import { renderHome, renderScreen } from './page.js';
import { isProtocol, proxy } from './proxy.js';
import { callerOf, contractValidator, InvalidScreen, isScreenName, loadScreen, loadSurfaces, type Screen } from './screens.js';

const MICRO_UI = /^[A-Z][A-Za-z0-9]*$/;

/** The files a page loads, by URL: the design system's own, read-only, and the build output. */
function assets(config: Config): Record<string, { path: string; type: string }> {
  return {
    '/assets/tokens.css': { path: join(config.root, 'mock/design/tokens.css'), type: 'text/css; charset=utf-8' },
    '/assets/kit.css': { path: join(config.root, 'mock/kit/kit.css'), type: 'text/css; charset=utf-8' },
    '/assets/shell.css': { path: join(config.dist, 'client/shell.css'), type: 'text/css; charset=utf-8' },
    '/assets/composer.js': { path: join(config.dist, 'client/composer.js'), type: 'text/javascript; charset=utf-8' },
  };
}

/** The audio the sound library may hold, by extension: anything else under /sounds/ is not served. */
const AUDIO: Record<string, string> = {
  '.ogg': 'audio/ogg',
  '.oga': 'audio/ogg',
  '.opus': 'audio/ogg',
  '.mp3': 'audio/mpeg',
  '.wav': 'audio/wav',
  '.flac': 'audio/flac',
  '.m4a': 'audio/mp4',
};

/**
 * The sound library's file a `/sounds/<file>` address names, with its type; `undefined` when the address
 * leaves the library (`..`, a hidden or empty segment, a backslash) or names a file that is not audio.
 */
function soundFile(config: Config, rest: string): { path: string; type: string } | undefined {
  let segments: string[];
  try {
    segments = rest.split('/').map(decodeURIComponent);
  } catch {
    return undefined;
  }
  if (segments.some((s) => s === '' || s.startsWith('.') || s.includes('\\') || s.includes('\0'))) return undefined;
  const type = AUDIO[(/\.[^.]+$/.exec(segments[segments.length - 1] ?? '')?.[0] ?? '').toLowerCase()];
  if (!type) return undefined;
  const library = resolve(config.root, 'mock/sounds');
  const path = resolve(library, ...segments);
  return path.startsWith(library + sep) ? { path, type } : undefined;
}

function send(res: ServerResponse, status: number, type: string, body: string): void {
  res.writeHead(status, { 'content-type': type, 'content-length': Buffer.byteLength(body) });
  res.end(body);
}

async function sendFile(res: ServerResponse, path: string, type: string): Promise<void> {
  if (!(await stat(path).then((s) => s.isFile(), () => false))) return send(res, 404, 'text/plain; charset=utf-8', 'not found\n');
  res.writeHead(200, { 'content-type': type });
  createReadStream(path)
    .on('error', () => res.destroy())
    .pipe(res);
}

/**
 * A sound file, whole or the `Range` of it a browser asks for (Safari needs the 206 to start an audio, and
 * every browser seeks with it); a HEAD gets the headers alone.
 */
async function sendSound(req: IncomingMessage, res: ServerResponse, { path, type }: { path: string; type: string }): Promise<void> {
  const info = await stat(path).catch(() => undefined);
  if (!info?.isFile()) return send(res, 404, 'text/plain; charset=utf-8', 'not found\n');
  const { size } = info;
  let [start, end, status] = [0, size - 1, 200];
  const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? '');
  if (range && (range[1] || range[2])) {
    if (range[1]) {
      start = Number(range[1]);
      if (range[2]) end = Math.min(Number(range[2]), size - 1);
    } else start = Math.max(size - Number(range[2]), 0);
    if (start > end) {
      res.writeHead(416, { 'content-range': `bytes */${size}` });
      return void res.end();
    }
    status = 206;
  }
  res.writeHead(status, {
    'content-type': type,
    'accept-ranges': 'bytes',
    'content-length': end - start + 1,
    ...(status === 206 ? { 'content-range': `bytes ${start}-${end}/${size}` } : {}),
  });
  if (req.method === 'HEAD' || size === 0) return void res.end();
  createReadStream(path, { start, end })
    .on('error', () => res.destroy())
    .pipe(res);
}

const CAMPAIGNS = 'campaign.listCampaigns';
const UPSTREAM_MS = 5000;

const attribute = (text: string): string => text.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** Whether a prop of the screen's layout reads the context's campaign. */
const readsCampaign = (screen: Screen): boolean => screen.layout.some(({ props }) => Object.values(props).includes('$ctx.campaign'));

/**
 * The campaign a screen shows when its address names none: among the ones with the latest `lastPlayed` (none
 * counts lowest) the one listed last, never the first row. `undefined` while the list is empty or cannot be read.
 */
async function defaultCampaign(config: Config): Promise<string | undefined> {
  try {
    const registry = JSON.parse(await readFile(join(config.root, 'queries/registry.json'), 'utf8')) as { queries?: Array<{ capability?: unknown; sha256?: unknown }> };
    const sha256 = registry.queries?.find((q) => q.capability === CAMPAIGNS)?.sha256;
    if (typeof sha256 !== 'string') return undefined;
    const answer = await fetch(new URL(`/capabilities/${CAMPAIGNS}`, config.serverUrl.origin), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ sha256, variables: {} }),
      signal: AbortSignal.timeout(UPSTREAM_MS),
    });
    if (!answer.ok) return undefined;
    const { data } = (await answer.json()) as { data?: unknown };
    if (!Array.isArray(data)) return undefined;
    let best: { id: string; lastPlayed: string } | undefined;
    for (const row of data as Array<{ id?: unknown; lastPlayed?: unknown } | null>) {
      if (typeof row?.id !== 'string' || !row.id) continue;
      const lastPlayed = typeof row.lastPlayed === 'string' ? row.lastPlayed : '';
      // `>=`: a tie goes to the one listed later.
      if (!best || lastPlayed >= best.lastPlayed) best = { id: row.id, lastPlayed };
    }
    return best?.id;
  } catch {
    return undefined;
  }
}

/** The page of a screen with what the shell's client reads from its head: the caller of its commands, and the default campaign. */
async function screenPage(config: Config, screen: Screen, url: URL, page: string): Promise<string> {
  const caller = callerOf(screen, await loadSurfaces(config.root), url.searchParams);
  const metas = [`<meta name="shell-caller" content="${attribute(JSON.stringify(caller))}">`];
  if (readsCampaign(screen) && !url.searchParams.get('campaign')) {
    const campaign = await defaultCampaign(config);
    if (campaign !== undefined) metas.push(`<meta name="shell-default-campaign" content="${attribute(campaign)}">`);
  }
  const end = page.indexOf('</head>');
  return end < 0 ? page : `${page.slice(0, end)}${metas.join('\n')}\n${page.slice(end)}`;
}

async function frozenScreens(config: Config): Promise<string[]> {
  const files = await readdir(join(config.root, 'apps/screens')).catch(() => []);
  return files
    .filter((f) => f.endsWith('.json'))
    .map((f) => f.slice(0, -'.json'.length))
    .filter(isScreenName)
    .sort();
}

export async function createApp(config: Config): Promise<RequestListener> {
  const validate = await contractValidator(config.root);
  const files = assets(config);

  const route = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
    // Only an origin-form target (`/path`): `//host/path` would name another host.
    const target = req.url ?? '/';
    if (!target.startsWith('/') || target.startsWith('//')) return send(res, 400, 'text/plain; charset=utf-8', 'bad request target\n');
    const url = new URL(target, 'http://shell');
    const { pathname } = url;
    if (isProtocol(pathname)) return proxy(req, res, config.serverUrl);

    if (req.method !== 'GET' && req.method !== 'HEAD') return send(res, 405, 'text/plain; charset=utf-8', 'method not allowed\n');

    if (pathname === '/') return send(res, 200, 'text/html; charset=utf-8', renderHome(await frozenScreens(config)));

    const screen = /^\/screens\/([^/]+)$/.exec(pathname);
    if (screen) {
      let name: string;
      try {
        name = decodeURIComponent(screen[1] ?? '');
      } catch {
        return send(res, 404, 'text/plain; charset=utf-8', 'no such screen\n');
      }
      try {
        const frozen = await loadScreen(config.root, name, validate);
        if (!frozen) return send(res, 404, 'text/plain; charset=utf-8', `no frozen screen "${name}"\n`);
        return send(res, 200, 'text/html; charset=utf-8', await screenPage(config, frozen, url, renderScreen(frozen)));
      } catch (error) {
        if (error instanceof InvalidScreen) return send(res, 500, 'text/plain; charset=utf-8', `${error.message}\n`);
        throw error;
      }
    }

    const microUi = /^\/micro-ui\/([A-Za-z0-9]+)\.js$/.exec(pathname);
    if (microUi && MICRO_UI.test(microUi[1] ?? '')) return sendFile(res, join(config.dist, 'micro-ui', `${microUi[1]}.js`), 'text/javascript; charset=utf-8');

    if (pathname.startsWith('/sounds/')) {
      const sound = soundFile(config, pathname.slice('/sounds/'.length));
      return sound ? sendSound(req, res, sound) : send(res, 404, 'text/plain; charset=utf-8', 'not found\n');
    }

    const file = files[pathname];
    if (file) return sendFile(res, file.path, file.type);

    return send(res, 404, 'text/plain; charset=utf-8', 'not found\n');
  };

  return (req, res) => {
    route(req, res).catch(() => {
      if (res.headersSent) return res.destroy();
      send(res, 500, 'text/plain; charset=utf-8', 'internal error\n');
    });
  };
}
