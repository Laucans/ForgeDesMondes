// The shell's HTTP surface: the home, a frozen screen at `/screens/<screen>`,
// the files its pages load, and the wire protocol forwarded to the server.

import { createReadStream } from 'node:fs';
import { readdir, stat } from 'node:fs/promises';
import type { IncomingMessage, RequestListener, ServerResponse } from 'node:http';
import { join } from 'node:path';
import type { Config } from './config.js';
import { renderHome, renderScreen } from './page.js';
import { isProtocol, proxy } from './proxy.js';
import { contractValidator, InvalidScreen, isScreenName, loadScreen } from './screens.js';

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
    const { pathname } = new URL(target, 'http://shell');
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
        return send(res, 200, 'text/html; charset=utf-8', renderScreen(frozen));
      } catch (error) {
        if (error instanceof InvalidScreen) return send(res, 500, 'text/plain; charset=utf-8', `${error.message}\n`);
        throw error;
      }
    }

    const microUi = /^\/micro-ui\/([A-Za-z0-9]+)\.js$/.exec(pathname);
    if (microUi && MICRO_UI.test(microUi[1] ?? '')) return sendFile(res, join(config.dist, 'micro-ui', `${microUi[1]}.js`), 'text/javascript; charset=utf-8');

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
