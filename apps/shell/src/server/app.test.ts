// The shell over real sockets: a stand-in for the server upstream, a scratch
// repository root for the frozen screens. Nothing outside the process.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer, request, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import Ajv2020 from 'ajv/dist/2020.js';
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

  describe('writes the caller and the default campaign into the head of a screen', () => {
    const SHA = 'ab'.repeat(32);
    const metaOf = (page: string, name: string): string | undefined => {
      const found = new RegExp(`<meta name="${name}" content="([^"]*)">`).exec(page)?.[1];
      return found?.replaceAll('&quot;', '"').replaceAll('&lt;', '<').replaceAll('&gt;', '>').replaceAll('&amp;', '&');
    };
    const listing = (campaigns: unknown): void => {
      answerWith = (_req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ asOf: 1, data: campaigns }));
      };
    };
    const row = (id: string, lastPlayed: string | null) => ({ id, name: id, type: 'campagne', lastPlayed });
    const defaultOf = async (path = '/screens/reader'): Promise<string | undefined> => metaOf(await (await fetch(`${base}${path}`)).text(), 'shell-default-campaign');

    before(async () => {
      await writeFile(
        join(root, 'surfaces.json'),
        JSON.stringify({ default: 'dm', surfaces: { dm: { by: 'dm', roles: ['role:approver'], permissions: [] }, player: { by: 'player', roles: [], permissions: [] } } }),
      );
      await mkdir(join(root, 'queries'), { recursive: true });
      await writeFile(join(root, 'queries/registry.json'), JSON.stringify({ queries: [{ query: 'Other', capability: 'x.y', sha256: 'ff' }, { query: 'ListCampaigns', capability: 'campaign.listCampaigns', sha256: SHA }] }));
      const withSurface = (screen: string, extra: object, layout: unknown) => ({ ...frozen(screen, layout), ...extra });
      await writeFile(join(root, 'apps/screens/player-view.json'), JSON.stringify(withSurface('player-view', { surface: 'player', pilot: { surface: 'dm', query: 'pilote' } }, [{ PlayerStage: {} }])));
      await writeFile(join(root, 'apps/screens/desk.json'), JSON.stringify(withSurface('desk', { surface: 'dm' }, [{ MediaLibrary: {} }])));
      await writeFile(join(root, 'apps/screens/bare.json'), JSON.stringify(frozen('bare', [{ MediaLibrary: {} }])));
      await writeFile(join(root, 'apps/screens/ghost.json'), JSON.stringify(withSurface('ghost', { surface: 'phantom' }, [{ MediaLibrary: {} }])));
      await writeFile(join(root, 'apps/screens/reader.json'), JSON.stringify(withSurface('reader', { surface: 'dm' }, [{ MediaLibrary: { campaign: '$ctx.campaign' } }])));
    });

    const callerOn = async (path: string): Promise<unknown> => JSON.parse(metaOf(await (await fetch(`${base}${path}`)).text(), 'shell-caller') ?? 'null');
    const DM = { by: 'dm', roles: ['role:approver'], permissions: [] };
    const PLAYER = { by: 'player', roles: [], permissions: [] };

    // AC10: the player's caller on player-view, the DM's with ?pilote=1 and on a DM screen, before </head>
    test("player-view acts as the player, unless the DM pilots the tab; a DM screen acts as the DM", async () => {
      assert.deepEqual(await callerOn('/screens/player-view'), PLAYER);
      assert.deepEqual(await callerOn('/screens/player-view?pilote=1'), DM);
      assert.deepEqual(await callerOn('/screens/player-view?pilote=0'), PLAYER);
      assert.deepEqual(await callerOn('/screens/player-view?pilot=1'), PLAYER);
      assert.deepEqual(await callerOn('/screens/desk'), DM);
      assert.deepEqual(await callerOn('/screens/desk?pilote=1'), DM);
      const page = await (await fetch(`${base}/screens/desk`)).text();
      assert.ok(page.indexOf('name="shell-caller"') < page.indexOf('</head>'));
      assert.equal(page.split('</head>').length, 2);
    });

    // AC7: a screen with no surface, or one surfaces.json does not declare, acts with no role
    test('a screen with no surface, or an undeclared one, acts with no role and no permission', async () => {
      assert.deepEqual(await callerOn('/screens/bare'), PLAYER);
      assert.deepEqual(await callerOn('/screens/ghost'), PLAYER);
    });

    test('a surfaces.json that is missing or unreadable gives the players\' caller on every screen', async () => {
      const path = join(root, 'surfaces.json');
      const kept = await readFile(path, 'utf8');
      try {
        for (const text of ['{not json', '[]', '{"surfaces":{"dm":{"by":"dm","roles":"role:approver"}}}', '{"surfaces":{"dm":{"by":"","roles":[]}}}']) {
          await writeFile(path, text);
          assert.deepEqual(await callerOn('/screens/desk'), PLAYER, text);
        }
        await rm(path);
        assert.deepEqual(await callerOn('/screens/desk?pilote=1'), PLAYER);
        assert.deepEqual(await callerOn('/screens/player-view?pilote=1'), PLAYER);
      } finally {
        await writeFile(path, kept);
      }
    });

    test('the caller meta cannot leave its attribute', async () => {
      const path = join(root, 'surfaces.json');
      const kept = await readFile(path, 'utf8');
      try {
        await writeFile(path, JSON.stringify({ surfaces: { dm: { by: '"><script>x()</script>', roles: [] } } }));
        const page = await (await fetch(`${base}/screens/desk`)).text();
        assert.doesNotMatch(page, /<script>x\(\)/);
      } finally {
        await writeFile(path, kept);
      }
    });

    // AC11: the default campaign meta, from campaign.listCampaigns read server-side
    test('the campaign with the latest lastPlayed is the default, and a tie goes to the one listed last', async () => {
      listing([row('a', '2026-03-01'), row('b', '2026-05-02'), row('c', null), row('d', '2026-04-01')]);
      assert.equal(await defaultOf(), 'b');
      listing([row('a', '2026-05-02'), row('b', '2026-05-02'), row('c', '2026-01-01')]);
      assert.equal(await defaultOf(), 'b');
      listing([row('a', '2026-05-02'), row('b', '2026-01-01'), row('c', '2026-05-02')]);
      assert.equal(await defaultOf(), 'c');
    });

    test('when no campaign was played, the one listed last is the default, never the first row', async () => {
      listing([row('a', null), row('b', null), row('c', null)]);
      assert.equal(await defaultOf(), 'c');
      listing([row('a', null), row('b', '2026-01-01'), row('c', null)]);
      assert.equal(await defaultOf(), 'b');
    });

    test('no default campaign is named for an empty or unreadable list', async () => {
      listing([]);
      assert.equal(await defaultOf(), undefined);
      listing(null);
      assert.equal(await defaultOf(), undefined);
      listing([{ name: 'no id' }, null, row('', null)]);
      assert.equal(await defaultOf(), undefined);
      answerWith = (_req, res) => {
        res.writeHead(500);
        res.end('boom');
      };
      assert.equal(await defaultOf(), undefined);
      answerWith = (_req, res) => res.end('not json');
      assert.equal(await defaultOf(), undefined);
      const registry = join(root, 'queries/registry.json');
      const kept = await readFile(registry, 'utf8');
      try {
        listing([row('a', null)]);
        await writeFile(registry, JSON.stringify({ queries: [] }));
        assert.equal(await defaultOf(), undefined);
        await rm(registry);
        assert.equal(await defaultOf(), undefined);
      } finally {
        await writeFile(registry, kept);
      }
    });

    test('the default campaign is asked of the server by its registered hash, and only for a screen that reads the context\'s campaign without one in its address', async () => {
      listing([row('a', null)]);
      seen.length = 0;
      assert.equal(await defaultOf(), 'a');
      assert.deepEqual(seen.map((s) => [s.method, s.url, s.body]), [['POST', '/capabilities/campaign.listCampaigns', JSON.stringify({ sha256: SHA, variables: {} })]]);
      seen.length = 0;
      assert.equal(await defaultOf('/screens/reader?campaign=ember'), undefined);
      assert.equal(await defaultOf('/screens/desk'), undefined);
      assert.equal(await defaultOf('/screens/player-view'), undefined);
      assert.equal(seen.length, 0);
      assert.equal(await defaultOf('/screens/reader?campaign='), 'a');
    });

    test('a default campaign cannot leave its attribute', async () => {
      listing([row('"><script>x()</script>', null)]);
      assert.doesNotMatch(await (await fetch(`${base}/screens/reader`)).text(), /<script>x\(\)/);
    });

    test('a server that is not there leaves the default unset, and the screen is still served', async () => {
      const dead = createServer();
      const url = await listen(dead);
      await close(dead);
      const lonely = createServer(await createApp(readConfig({ PORT: '0', SERVER_URL: url }, root)));
      const lonelyBase = await listen(lonely);
      const answer = await fetch(`${lonelyBase}/screens/reader`);
      assert.equal(answer.status, 200);
      assert.equal(metaOf(await answer.text(), 'shell-default-campaign'), undefined);
      await close(lonely);
    });
  });

  describe('the product declares its surfaces', () => {
    const repository = repositoryRoot();
    const validator = (schema: string) => new Ajv2020.default({ allErrors: true, strict: false }).compile(JSON.parse(readFileSync(join(repository, 'contracts', schema), 'utf8')) as object);
    const surfaces = JSON.parse(readFileSync(join(repository, 'surfaces.json'), 'utf8')) as { default: string; surfaces: Record<string, { by: string; roles: string[]; permissions: string[]; description?: string }> };

    // AC1: surfaces.json validates against contract S, dm and player as declared
    test('surfaces.json is valid (contract S): the DM approves, the player holds nothing', () => {
      const validate = validator('s-surfaces.schema.json');
      assert.ok(validate(surfaces), JSON.stringify(validate.errors));
      assert.equal(surfaces.default, 'dm');
      assert.deepEqual(surfaces.surfaces.dm, { by: 'dm', roles: ['role:approver'], permissions: [], description: surfaces.surfaces.dm?.description });
      assert.deepEqual(surfaces.surfaces.player, { by: 'player', roles: [], permissions: [], description: surfaces.surfaces.player?.description });
    });

    // AC2: every frozen screen declares a surface; player-view the player's with the DM as pilot
    test('every frozen screen declares its surface (contract E): the DM\'s, but the player view\'s', () => {
      const validate = validator('e-screen-composition.schema.json');
      const names = readdirSync(join(repository, 'apps/screens')).filter((f) => f.endsWith('.json'));
      assert.ok(names.length > 0);
      for (const file of names) {
        const screen = JSON.parse(readFileSync(join(repository, 'apps/screens', file), 'utf8')) as { surface?: string; pilot?: unknown };
        assert.ok(validate(screen), `${file}: ${JSON.stringify(validate.errors)}`);
        if (file === 'player-view.json') {
          assert.equal(screen.surface, 'player');
          assert.deepEqual(screen.pilot, { surface: 'dm', query: 'pilote' });
        } else {
          assert.equal(screen.surface, 'dm', file);
          assert.equal(screen.pilot, undefined, file);
        }
        assert.ok(screen.surface && screen.surface in surfaces.surfaces, file);
      }
    });

    // AC3: the dm-view page, which names its frozen screen, declares surface dm after its objective
    test('the dm-view page declares the DM\'s surface, on a line after its objective (contract N)', () => {
      const text = readFileSync(join(repository, 'mock/pages.json'), 'utf8');
      const validate = validator('n-mock-pages.schema.json');
      const pages = JSON.parse(text) as { pages: Array<{ path: string; screen?: string; surface?: string }> };
      assert.ok(validate(pages), JSON.stringify(validate.errors));
      const page = pages.pages.find((p) => p.path === 'pages/dm-view.html');
      assert.equal(page?.screen, 'dm-view');
      assert.equal(page?.surface, 'dm');
      const lines = text.split('\n');
      const objective = lines.findIndex((l) => l.includes('"objective": "Le DM fait rouler la séance'));
      assert.match(lines[objective + 1] ?? '', /^\s+"surface": "dm",$/);
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

    describe('serves the sound library at /sounds/<file>', () => {
      let library: Server;
      let at: string;

      before(async () => {
        library = createServer(await createApp(readConfig({ PORT: '0', SERVER_URL: 'http://127.0.0.1:1' }, repositoryRoot())));
        at = await listen(library);
      });
      after(() => close(library));

      /** The length of an Ogg Opus file, from the granule position of its last page (48 kHz). */
      const seconds = (file: Buffer): number => {
        const last = file.lastIndexOf('OggS');
        assert.ok(last >= 0, 'an Ogg file');
        return Number(file.readBigUInt64LE(last + 6)) / 48000;
      };

      // AC1: GET /sounds/foule-taverne.ogg answers 200 with an audio type
      test('answers 200 with an audio type for a file of the library', async () => {
        const answer = await fetch(`${at}/sounds/foule-taverne.ogg`);
        assert.equal(answer.status, 200);
        assert.equal(answer.headers.get('content-type'), 'audio/ogg');
        assert.equal(Buffer.from(await answer.arrayBuffer()).subarray(0, 4).toString(), 'OggS');
      });

      // The soundbox plays each sound of sounds.json (R16 needs two of them), so each has a file at least as long
      test('holds a playable file, as long as its duration, for each sound of sounds.json', async () => {
        const { sounds } = JSON.parse(readFileSync(join(repositoryRoot(), 'mock/fixtures/sounds.json'), 'utf8')) as {
          sounds: Array<{ file: string; duration: number }>;
        };
        assert.ok(sounds.length > 0);
        for (const { file, duration } of sounds) {
          const answer = await fetch(`${at}/sounds/${encodeURIComponent(file)}`);
          assert.equal(answer.status, 200, file);
          assert.ok(seconds(Buffer.from(await answer.arrayBuffer())) >= duration, `${file} lasts at least ${duration} s`);
        }
      });

      test('answers a Range with the 206 a browser plays audio from, and a HEAD with the headers alone', async () => {
        const url = `${at}/sounds/foule-taverne.ogg`;
        const whole = Buffer.from(await (await fetch(url)).arrayBuffer());
        const head = await fetch(url, { method: 'HEAD' });
        assert.equal(head.status, 200);
        assert.equal(head.headers.get('accept-ranges'), 'bytes');
        assert.equal(head.headers.get('content-length'), String(whole.length));
        assert.equal((await head.arrayBuffer()).byteLength, 0);

        const part = await fetch(url, { headers: { range: 'bytes=0-1' } });
        assert.equal(part.status, 206);
        assert.equal(part.headers.get('content-range'), `bytes 0-1/${whole.length}`);
        assert.deepEqual(Buffer.from(await part.arrayBuffer()), whole.subarray(0, 2));

        const open = await fetch(url, { headers: { range: 'bytes=10-' } });
        assert.equal(open.headers.get('content-range'), `bytes 10-${whole.length - 1}/${whole.length}`);
        assert.deepEqual(Buffer.from(await open.arrayBuffer()), whole.subarray(10));

        const tail = await fetch(url, { headers: { range: 'bytes=-4' } });
        assert.equal(tail.status, 206);
        assert.deepEqual(Buffer.from(await tail.arrayBuffer()), whole.subarray(whole.length - 4));

        const beyond = await fetch(url, { headers: { range: `bytes=${whole.length}-` } });
        assert.equal(beyond.status, 416);
        assert.equal(beyond.headers.get('content-range'), `bytes */${whole.length}`);
      });

      test('answers 404 for a file the library lacks', async () => {
        assert.equal((await fetch(`${at}/sounds/absent.ogg`)).status, 404);
      });

      test('never leaves the library, nor serves what is not audio', async () => {
        const net = (path: string) =>
          new Promise<number>((done, fail) => {
            const { port } = new URL(at);
            request({ host: '127.0.0.1', port, path }, (res) => (res.resume(), done(res.statusCode ?? 0))).on('error', fail).end();
          });
        for (const path of [
          '/sounds/../../CLAUDE.md',
          '/sounds/%2e%2e/%2e%2e/CLAUDE.md',
          '/sounds/..%2f..%2fCLAUDE.md',
          '/sounds/%5C..%5Cfoule-taverne.ogg',
          '/sounds/foule-taverne.ogg%00.txt',
          '/sounds/.hidden.ogg',
          '/sounds//foule-taverne.ogg',
          '/sounds/%E0%A4%A.ogg',
          '/sounds/',
        ]) {
          assert.equal(await net(path), 404, path);
        }
        // An extension that is not audio is no sound, whatever the library holds.
        assert.equal((await fetch(`${at}/sounds/README.md`)).status, 404);
        assert.equal((await fetch(`${at}/sounds/foule-taverne.ogg`, { method: 'POST' })).status, 405);
      });
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
