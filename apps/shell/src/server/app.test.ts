// The shell over real sockets: a stand-in for the server upstream, a scratch
// repository root for the frozen screens. Nothing outside the process.

import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer, request, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { createApp } from './app.js';
import { readConfig, repositoryRoot } from './config.js';

const listen = (server: Server): Promise<string> =>
  new Promise((done) => server.listen(0, '127.0.0.1', () => done(`http://127.0.0.1:${(server.address() as AddressInfo).port}`)));

const close = (server: Server): Promise<void> =>
  new Promise((done) => {
    server.close(() => done());
    server.closeAllConnections();
  });

const frozen = (screen: string, layout: unknown) => ({ screen, generatedFrom: 'a goal', version: 3, layout });

describe('the shell', () => {
  let root: string;
  let upstream: Server;
  let shell: Server;
  let base: string;
  const seen: Array<{ method?: string; url?: string; body: string; headers: IncomingMessage['headers'] }> = [];
  let answerWith: (req: IncomingMessage, res: ServerResponse) => void;

  before(async () => {
    root = await mkdtemp(join(tmpdir(), 'shell-'));
    await mkdir(join(root, 'contracts'));
    await cp(join(repositoryRoot(), 'contracts/e-screen-composition.schema.json'), join(root, 'contracts/e-screen-composition.schema.json'));
    await mkdir(join(root, 'apps/screens'), { recursive: true });
    await mkdir(join(root, 'mock/design'), { recursive: true });
    await writeFile(join(root, 'mock/design/tokens.css'), ':root { --space-1: 4px; }\n');
    await writeFile(join(root, 'apps/screens/medias.json'), JSON.stringify(frozen('medias', [{ MediaLibrary: { campaign: '$ctx.campaign' } }, { GridSettings: {} }])));
    await writeFile(join(root, 'apps/screens/broken.json'), JSON.stringify({ screen: 'broken', version: 0, layout: [] }));
    await writeFile(join(root, 'apps/screens/Bad Name.json'), '{}');
    await writeFile(join(root, 'apps/screens/other.json'), JSON.stringify(frozen('medias', [{ MediaLibrary: {} }])));

    upstream = createServer((req, res) => {
      const chunks: Buffer[] = [];
      req.on('data', (c: Buffer) => chunks.push(c));
      req.on('end', () => {
        seen.push({ method: req.method, url: req.url, body: Buffer.concat(chunks).toString(), headers: req.headers });
        answerWith(req, res);
      });
    });
    const serverUrl = await listen(upstream);
    shell = createServer(await createApp(readConfig({ PORT: '0', SERVER_URL: serverUrl }, root)));
    base = await listen(shell);
  });

  after(async () => {
    await close(shell);
    await close(upstream);
    await rm(root, { recursive: true, force: true });
  });

  describe('forwards the wire protocol to SERVER_URL', () => {
    test('status, headers and body come back as the server sent them', async () => {
      seen.length = 0;
      answerWith = (_req, res) => {
        res.writeHead(409, { 'content-type': 'application/json', 'x-from': 'server' });
        res.end('{"error":"refused"}');
      };
      const answer = await fetch(`${base}/capabilities/campaign.listCampaigns?x=1`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"variables":{"a":1}}' });
      assert.equal(answer.status, 409);
      assert.equal(answer.headers.get('x-from'), 'server');
      assert.equal(await answer.text(), '{"error":"refused"}');
      assert.deepEqual(seen.map((s) => [s.method, s.url, s.body]), [['POST', '/capabilities/campaign.listCampaigns?x=1', '{"variables":{"a":1}}']]);
      assert.equal(seen[0]?.headers['content-type'], 'application/json');
    });

    test('every path of the protocol reaches the server', async () => {
      answerWith = (_req, res) => res.end('ok');
      for (const path of ['/health', '/capabilities/a.b', '/commands', '/commands/1/confirm', '/holds', '/holds/1/renew', '/reviews/1', '/dataVersion']) {
        seen.length = 0;
        const answer = await fetch(`${base}${path}`);
        assert.equal(await answer.text(), 'ok', path);
        assert.equal(seen[0]?.url, path);
      }
    });

    test('a request target that names another host is refused, never forwarded', async () => {
      seen.length = 0;
      answerWith = (_req, res) => res.end('ok');
      const { port } = new URL(base);
      const status = await new Promise<number>((done, fail) => {
        request({ host: '127.0.0.1', port, path: '//evil.example/capabilities/x' }, (res) => {
          res.resume();
          done(res.statusCode ?? 0);
        }).on('error', fail).end();
      });
      assert.equal(status, 400);
      assert.equal(seen.length, 0);
    });

    test('a stream is delivered as it comes, not once it ends', async () => {
      answerWith = (_req, res) => {
        res.writeHead(200, { 'content-type': 'text/event-stream' });
        res.write('event: dataVersion\ndata: {"dataVersion":2}\n\n');
        // The stream stays open: only the client going away ends it.
      };
      const answer = await fetch(`${base}/dataVersion`);
      assert.equal(answer.headers.get('content-type'), 'text/event-stream');
      const reader = (answer.body as ReadableStream<Uint8Array>).getReader();
      const first = await reader.read();
      assert.match(new TextDecoder().decode(first.value), /"dataVersion":2/);
      await reader.cancel();
    });

    test('a server that does not answer is a 502, not a reply of the shell', async () => {
      const dead = createServer();
      const url = await listen(dead);
      await close(dead);
      const lonely = createServer(await createApp(readConfig({ PORT: '0', SERVER_URL: url }, root)));
      const lonelyBase = await listen(lonely);
      const answer = await fetch(`${lonelyBase}/health`);
      assert.equal(answer.status, 502);
      assert.deepEqual(await answer.json(), { error: 'server-unreachable' });
      await close(lonely);
    });
  });

  describe('replays a frozen screen at /screens/<screen>', () => {
    test('no such frozen screen is a 404', async () => {
      assert.equal((await fetch(`${base}/screens/x`)).status, 404);
    });

    test('a name that is not a screen name never reaches the disk', async () => {
      for (const name of ['..%2F..%2Fcontracts%2Fe-screen-composition', 'Medias', 'medias.json']) {
        assert.equal((await fetch(`${base}/screens/${name}`)).status, 404, name);
      }
    });

    test('a malformed escape in a name is no screen, not a fault', async () => {
      assert.equal((await fetch(`${base}/screens/%E0%A4%A`)).status, 404);
    });

    test('the page lays out the Micro-UIs the screen names, in its order, with its props', async () => {
      const page = await (await fetch(`${base}/screens/medias`)).text();
      const placed = [...page.matchAll(/data-component="(\w+)" data-props="([^"]*)"/g)].map((m) => [m[1], (m[2] ?? '').replaceAll('&quot;', '"')]);
      assert.deepEqual(placed, [
        ['MediaLibrary', '{"campaign":"$ctx.campaign"}'],
        ['GridSettings', '{}'],
      ]);
      assert.match(page, /data-screen="medias" data-version="3"/);
    });

    test('is replayed as frozen: the same page every time, the file untouched', async () => {
      const path = join(root, 'apps/screens/medias.json');
      const before = await readFile(path, 'utf8');
      const first = await (await fetch(`${base}/screens/medias`)).text();
      const second = await (await fetch(`${base}/screens/medias`)).text();
      assert.equal(first, second);
      assert.equal(await readFile(path, 'utf8'), before);
    });

    test('a file that breaks contract E, or freezes another screen, is a 500 that says why', async () => {
      const broken = await fetch(`${base}/screens/broken`);
      assert.equal(broken.status, 500);
      assert.match(await broken.text(), /breaks contract E/);
      const other = await fetch(`${base}/screens/other`);
      assert.equal(other.status, 500);
      assert.match(await other.text(), /freezes the screen "medias"/);
    });

    test('props are escaped: a value cannot leave its attribute', async () => {
      await writeFile(join(root, 'apps/screens/hostile.json'), JSON.stringify(frozen('hostile', [{ Note: { text: '"><script>x()</script>' } }])));
      const page = await (await fetch(`${base}/screens/hostile`)).text();
      assert.doesNotMatch(page, /<script>x\(\)/);
    });
  });

  describe('serves its own pages', () => {
    test('GET / answers 200 and lists the frozen screens', async () => {
      const answer = await fetch(`${base}/`);
      assert.equal(answer.status, 200);
      const home = await answer.text();
      assert.match(home, /href="\/screens\/medias"/);
      // Only what a screen can be called is advertised.
      assert.doesNotMatch(home, /Bad Name/);
    });

    test('loads the design system\'s tokens as they are', async () => {
      const answer = await fetch(`${base}/assets/tokens.css`);
      assert.equal(answer.headers.get('content-type'), 'text/css; charset=utf-8');
      assert.equal(await answer.text(), ':root { --space-1: 4px; }\n');
    });

    test('a file outside the allowlist, or a write, is refused', async () => {
      assert.equal((await fetch(`${base}/assets/../../CLAUDE.md`)).status, 404);
      assert.equal((await fetch(`${base}/micro-ui/Missing.js`)).status, 404);
      assert.equal((await fetch(`${base}/screens/medias`, { method: 'POST' })).status, 405);
    });
  });
});

describe('the configuration', () => {
  test('needs SERVER_URL and a real PORT', () => {
    assert.throws(() => readConfig({ PORT: '8791' }, '/r'), /SERVER_URL/);
    assert.throws(() => readConfig({ PORT: 'abc', SERVER_URL: 'http://x' }, '/r'), /PORT/);
    assert.equal(readConfig({ PORT: '8791', SERVER_URL: 'http://x:1' }, '/r').port, 8791);
  });

  test('an empty PORT is the default one, not a random port; the shell listens on loopback unless told', () => {
    const config = readConfig({ PORT: ' ', SERVER_URL: 'http://x:1' }, '/r');
    assert.equal(config.port, 8791);
    assert.equal(config.host, '127.0.0.1');
    assert.equal(readConfig({ HOST: '0.0.0.0', SERVER_URL: 'http://x:1' }, '/r').host, '0.0.0.0');
  });
});
