import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createShell, ProtocolError, type Page } from './shell.js';

const urlOf = (input: RequestInfo | URL): string => (typeof input === 'string' ? input : input instanceof URL ? input.href : input.url);
const answer = (status: number, body: unknown): Response => new Response(JSON.stringify(body), { status });

test('a Capability is called by identifier with its variables, and answers what the server answered', async () => {
  const calls: Array<[string, RequestInit | undefined]> = [];
  const shell = createShell(async (input, init) => {
    calls.push([urlOf(input), init]);
    return answer(200, { asOf: 4, data: ['a'] });
  });
  assert.deepEqual(await shell.capability('campaign.listCampaigns', 'ab12', { archived: false }), { asOf: 4, data: ['a'] });
  assert.equal(calls[0]?.[0], '/capabilities/campaign.listCampaigns');
  assert.equal(calls[0]?.[1]?.method, 'POST');
  assert.equal(calls[0]?.[1]?.body, '{"sha256":"ab12","variables":{"archived":false}}');
});

test('an answer that is not a success is an error carrying the server\'s status and body', async () => {
  const shell = createShell(async () => answer(404, { error: 'not-found' }));
  await assert.rejects(shell.capability('nope.nope', 'ab12'), (error: unknown) => error instanceof ProtocolError && error.status === 404 && JSON.stringify(error.body) === '{"error":"not-found"}');
});

test('an error that is not JSON still carries the status', async () => {
  const shell = createShell(async () => new Response('not found\n', { status: 404 }));
  await assert.rejects(shell.commandResult('x'), (error: unknown) => error instanceof ProtocolError && error.status === 404 && error.body === 'not found\n');
});

test('commands go to /commands and are looked up by id', async () => {
  const urls: string[] = [];
  const shell = createShell(async (input) => {
    urls.push(urlOf(input));
    return answer(200, { result: {} });
  });
  await shell.command({ dataCapability: 'x.y' });
  await shell.commandResult('a b');
  assert.deepEqual(urls, ['/commands', '/commands/a%20b']);
});

/** A shell whose server answers `POST /commands` with `submitted`, then each `GET /commands/<id>` with the next of `states`; every URL asked is kept. */
function writer(submitted: { status: number; state?: string }, states: string[], polls = 50): { shell: ReturnType<typeof createShell>; urls: string[] } {
  const urls: string[] = [];
  const shell = createShell(
    async (input) => {
      const url = urlOf(input);
      urls.push(url);
      if (url === '/commands') return answer(submitted.status, { commandId: 'c1', result: { commandId: 'c1', status: submitted.state } });
      if (url === '/commands/c1') return answer(200, { result: { commandId: 'c1', status: states.shift() ?? 'queued' } });
      return answer(200, { asOf: 1, data: [] });
    },
    pageOf('http://shell/screens/x', metaOf(DM)),
    { sleep: async () => undefined, everyMs: 0, polls },
  );
  return { shell, urls };
}

test("a read asked right after a command of the page waits for it to be applied: the page reads its own writes", async () => {
  const { shell, urls } = writer({ status: 202, state: 'queued' }, ['confirmed', 'applied']);
  const saved = shell.command({ dataCapability: 'x.save' });
  await shell.capability('x.list', 'ab12');
  assert.deepEqual(urls, ['/commands', '/commands/c1', '/commands/c1', '/capabilities/x.list']);
  await saved;
  // Settled: the next read asks at once.
  await shell.capability('x.list', 'ab12');
  assert.deepEqual(urls.slice(4), ['/capabilities/x.list']);
});

test('a read never waits for a command a person must act on, nor for one refused', async () => {
  for (const state of ['awaiting_confirmation', 'awaiting_review', 'parked', 'applied']) {
    const { shell, urls } = writer({ status: 202, state }, []);
    void shell.command({ dataCapability: 'x.save' });
    await shell.capability('x.list', 'ab12');
    assert.deepEqual(urls, ['/commands', '/capabilities/x.list'], state);
  }
  const { shell, urls } = writer({ status: 422 }, []);
  const refused = shell.command({ dataCapability: 'x.save' });
  await shell.capability('x.list', 'ab12');
  await assert.rejects(refused, (error: unknown) => error instanceof ProtocolError && error.status === 422);
  assert.deepEqual(urls, ['/commands', '/capabilities/x.list']);
});

test('a read waits for a command that does not settle only so long', async () => {
  const { shell, urls } = writer({ status: 202, state: 'queued' }, [], 3);
  void shell.command({ dataCapability: 'x.save' });
  await shell.capability('x.list', 'ab12');
  assert.deepEqual(urls, ['/commands', '/commands/c1', '/commands/c1', '/commands/c1', '/capabilities/x.list']);
});

test('the data version is polled, and told only when it moved, until stopped', async () => {
  const served = [1, 1, 2];
  let polls = 0;
  const shell = createShell(async () => answer(200, { dataVersion: served[Math.min(polls++, served.length - 1)] }));
  const versions: number[] = [];
  const stop = shell.onDataVersion((v) => versions.push(v), 5);
  await new Promise((done) => setTimeout(done, 80));
  stop();
  const settled = polls;
  await new Promise((done) => setTimeout(done, 30));
  assert.deepEqual(versions, [1, 2]);
  assert.equal(polls, settled);
});

test('the data version is asked again only once the last answer is in', async () => {
  let open = 0;
  let overlapped = false;
  const shell = createShell(async () => {
    open += 1;
    overlapped ||= open > 1;
    await new Promise((done) => setTimeout(done, 30));
    open -= 1;
    return answer(200, { dataVersion: 1 });
  });
  const stop = shell.onDataVersion(() => undefined, 5);
  await new Promise((done) => setTimeout(done, 120));
  stop();
  assert.equal(overlapped, false);
});

// The page of a test: its address, the metas the shell's server wrote, the files its own fetch holds (by URL; others are 404).
const pageOf = (href: string, metas: Record<string, string> = {}, files: Record<string, unknown> = {}, asked: string[] = []): Page => ({
  href,
  meta: (name) => metas[name],
  fetch: async (input) => {
    const url = urlOf(input);
    asked.push(url);
    return url in files ? answer(200, files[url]) : new Response('not found', { status: 404 });
  },
});

/** The bodies a shell POSTs to /commands, over a page. */
function sendOn(page: Page, body: unknown): Promise<unknown> {
  const sent: unknown[] = [];
  const shell = createShell(async (input, init) => {
    if (urlOf(input) === '/commands') sent.push(JSON.parse(init?.body as string));
    return answer(200, { result: {} });
  }, page);
  return shell.command(body).then(() => sent[0]);
}

const DM = { by: 'dm', roles: ['role:approver'], permissions: [] };
const PLAYER = { by: 'player', roles: [], permissions: [] };
const metaOf = (caller: unknown): Record<string, string> => ({ 'shell-caller': JSON.stringify(caller) });

// AC4: a player-view page without ?pilote=1 sends the player's caller and keeps the Micro-UI's from
test("the caller the shell's server wrote replaces what the Micro-UI sent, and its from stays", async () => {
  const body = await sendOn(pageOf('http://shell/screens/player-view', metaOf(PLAYER)), { dataCapability: 'x.y', caller: { by: 'dm', from: 'PlayerStage', roles: ['role:approver'] } });
  assert.deepEqual(body, { dataCapability: 'x.y', caller: { by: 'player', from: 'PlayerStage', roles: [], permissions: [] } });
});

// AC5: the same page with ?pilote=1 sends the DM's caller
// AC6: on a DM screen, the roles a Micro-UI sends are replaced by the DM's
test("the DM's caller is the surface's, whatever roles the Micro-UI sent", async () => {
  const body = (await sendOn(pageOf('http://shell/screens/player-view?pilote=1', metaOf(DM)), { caller: { by: 'player', from: 'PlayerStage', roles: ['role:other', 'role:approver'], permissions: ['x'] } })) as { caller: unknown };
  assert.deepEqual(body.caller, { by: 'dm', from: 'PlayerStage', roles: ['role:approver'], permissions: [] });
});

// AC7: a meta that is missing the mock-up cannot resolve, or that is not a whole caller, fails closed
test('any doubt about the caller is the players\': no role, no permission', async () => {
  const unwhole = [{}, { by: 'dm' }, { by: '', roles: [] }, { by: 'dm', roles: 'role:approver' }, { by: 'dm', roles: [1] }, { by: 'dm', roles: [], permissions: [2] }, null, 'dm'];
  for (const caller of unwhole) {
    const body = (await sendOn(pageOf('http://shell/screens/x', metaOf(caller)), { caller: { by: 'dm', from: 'A', roles: ['role:approver'] } })) as { caller: unknown };
    assert.deepEqual(body.caller, { by: 'player', from: 'A', roles: [], permissions: [] }, JSON.stringify(caller));
  }
  const broken = (await sendOn(pageOf('http://shell/screens/x', { 'shell-caller': '{not json' }), { caller: { roles: ['role:approver'] } })) as { caller: unknown };
  assert.deepEqual(broken.caller, PLAYER);
  // No meta and no mock-up to read: the address is no page, the files are not there.
  const nothing = (await sendOn(pageOf(''), { caller: { by: 'dm', roles: ['role:approver'] } })) as { caller: unknown };
  assert.deepEqual(nothing.caller, PLAYER);
});

test('a body that is not an object goes as it is', async () => {
  assert.equal(await sendOn(pageOf('', metaOf(DM)), 'text'), 'text');
  assert.deepEqual(await sendOn(pageOf('', metaOf(DM)), [1]), [1]);
});

const MOCKUP = 'http://mock/mock/pages/player-view.html';
const files = (pilot = true): Record<string, unknown> => ({
  'http://mock/mock/pages.json': { pages: [{ path: 'pages/dm-view.html', surface: 'dm' }, { path: 'pages/player-view.html', screen: 'player-view' }, { path: 'pages/orphan.html', screen: 'orphan' }, { path: 'pages/bare.html' }] },
  'http://mock/mock/__product/apps/screens/player-view.json': { screen: 'player-view', surface: 'player', ...(pilot ? { pilot: { surface: 'dm', query: 'pilote' } } : {}) },
  'http://mock/mock/__product/surfaces.json': { default: 'dm', surfaces: { dm: { ...DM }, player: { ...PLAYER } } },
});

// AC8: in the mock-up the caller is read from ../pages.json, then surfaces.json and the screen from ../__product/, else from ../../, with the page's own fetch
test("in the mock-up, the caller comes from the page's screen, read with the page's own fetch", async () => {
  const asked: string[] = [];
  const body = (await sendOn(pageOf(MOCKUP, {}, files(), asked), { caller: { from: 'PlayerStage' } })) as { caller: unknown };
  assert.deepEqual(body.caller, { from: 'PlayerStage', ...PLAYER });
  assert.deepEqual(asked, ['http://mock/mock/pages.json', 'http://mock/mock/__product/apps/screens/player-view.json', 'http://mock/mock/__product/surfaces.json']);
  const piloted = (await sendOn(pageOf(`${MOCKUP}?pilote=1`, {}, files()), {})) as { caller: unknown };
  assert.deepEqual(piloted.caller, DM);
  // The query is the pilot's own, and only the value 1 grants it.
  for (const href of [`${MOCKUP}?pilote=0`, `${MOCKUP}?pilot=1`, `${MOCKUP}?pilote`]) assert.deepEqual(((await sendOn(pageOf(href, {}, files()), {})) as { caller: unknown }).caller, PLAYER, href);
});

test('in the mock-up, a file the factory view does not serve is read from the repository root', async () => {
  const asked: string[] = [];
  const held = files();
  const moved = Object.fromEntries(Object.entries(held).map(([url, file]) => [url.replace('/mock/__product/', '/'), file]));
  const body = (await sendOn(pageOf(`${MOCKUP}?pilote=1`, {}, moved, asked), {})) as { caller: unknown };
  assert.deepEqual(body.caller, DM);
  assert.deepEqual(asked, [
    'http://mock/mock/pages.json',
    'http://mock/mock/__product/apps/screens/player-view.json',
    'http://mock/apps/screens/player-view.json',
    'http://mock/mock/__product/surfaces.json',
    'http://mock/surfaces.json',
  ]);
});

test('a page no frozen screen names acts as the surface its page declares', async () => {
  const body = (await sendOn(pageOf('http://mock/mock/pages/dm-view.html', {}, files()), {})) as { caller: unknown };
  assert.deepEqual(body.caller, DM);
});

// AC7: a screen with no surface, a missing surfaces.json, an unreadable one, fail closed
test('in the mock-up, a screen with no surface, a missing or unreadable surfaces.json, or an unknown page fail closed', async () => {
  const held = files();
  const without = { ...held, 'http://mock/mock/__product/apps/screens/player-view.json': { screen: 'player-view' } };
  const missing = Object.fromEntries(Object.entries(held).filter(([url]) => !url.endsWith('surfaces.json')));
  const unreadable = { ...held, 'http://mock/mock/__product/surfaces.json': 'not a declaration' };
  const unknown = { ...held, 'http://mock/mock/__product/surfaces.json': { default: 'dm', surfaces: { dm: { ...DM } } } };
  const cases: Array<[string, string, Record<string, unknown>]> = [
    ['no surface', `${MOCKUP}?pilote=1`, without],
    ['no surfaces.json', MOCKUP, missing],
    ['unreadable surfaces.json', `${MOCKUP}?pilote=1`, unreadable],
    ['a surface it does not declare', MOCKUP, unknown],
    ['no pages.json', MOCKUP, {}],
    ['a page the mock-up does not list', 'http://mock/mock/pages/other.html', held],
    ['a screen no file holds', 'http://mock/mock/pages/orphan.html', held],
    ['a page with neither screen nor surface', 'http://mock/mock/pages/bare.html', held],
  ];
  for (const [what, href, served] of cases) {
    const body = (await sendOn(pageOf(href, {}, served), { caller: { roles: ['role:approver'] } })) as { caller: unknown };
    assert.deepEqual(body.caller, PLAYER, what);
  }
});

test('the caller of a page is resolved once, for all its commands', async () => {
  const asked: string[] = [];
  const sent: unknown[] = [];
  const shell = createShell(async (input, init) => {
    sent.push(JSON.parse(init?.body as string));
    return answer(200, { result: urlOf(input) });
  }, pageOf(MOCKUP, {}, files(), asked));
  await Promise.all([shell.command({ n: 1 }), shell.command({ n: 2 })]);
  await shell.command({ n: 3 });
  assert.equal(sent.length, 3);
  assert.equal(asked.length, 3);
});

test('a file the mock-up could not be read is asked again by the next command, failing closed meanwhile', async () => {
  let up = false;
  const held = files();
  const page: Page = {
    href: `${MOCKUP}?pilote=1`,
    meta: () => undefined,
    fetch: async (input) => (up ? answer(200, held[urlOf(input)] ?? null) : Promise.reject(new TypeError('offline'))),
  };
  const sent: unknown[] = [];
  const shell = createShell(async (_input, init) => {
    sent.push((JSON.parse(init?.body as string) as { caller: unknown }).caller);
    return answer(200, {});
  }, page);
  await shell.command({});
  up = true;
  await shell.command({});
  await shell.command({});
  assert.deepEqual(sent, [PLAYER, DM, DM]);
});

// AC9: an address campaign holds against a later context write; a default seed does not
test("the address' campaign is held against a later write of the context; the default one is replaced", () => {
  const named = createShell(async () => answer(200, {}), pageOf('http://shell/screens/campagne?campaign=ember', { 'shell-default-campaign': 'dflt' }));
  assert.equal(named.context.get('campaign'), 'ember');
  named.context.set('campaign', 'other');
  assert.equal(named.context.get('campaign'), 'ember');
  named.context.set('scene', 's1');
  assert.equal(named.context.get('scene'), 's1');

  const seeded = createShell(async () => answer(200, {}), pageOf('http://shell/screens/campagne', { 'shell-default-campaign': 'dflt' }));
  assert.equal(seeded.context.get('campaign'), 'dflt');
  seeded.context.set('campaign', 'live');
  assert.equal(seeded.context.get('campaign'), 'live');

  // An empty campaign in the address names none; no meta seeds nothing.
  const empty = createShell(async () => answer(200, {}), pageOf('http://shell/screens/campagne?campaign=', { 'shell-default-campaign': 'dflt' }));
  assert.equal(empty.context.get('campaign'), 'dflt');
  assert.equal(createShell(async () => answer(200, {}), pageOf('http://shell/screens/campagne')).context.get('campaign'), undefined);
});
