import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createShell, ProtocolError } from './shell.js';

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
