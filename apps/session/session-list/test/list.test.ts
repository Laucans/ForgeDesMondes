import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Env } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));

interface Row {
  id: string;
  order: number;
  title: string;
  date: string;
  status: string;
  present: string[];
  summary: string | null;
}
interface Person {
  id: string;
  name: string;
  kind: string;
}

/** The mock-up's fixtures of the two Capabilities: what the list is opened on. */
const SESSIONS = fixture('session.listSessions') as Row[];
const PEOPLE = fixture('campaign.listCharacters') as Person[];
const PCS = PEOPLE.filter((p) => p.kind === 'pj').map((p) => p.id);

interface Sent {
  dataCapability: string;
  payload: { campaign: string; title: string; plannedOn: string; present: string[] };
}

/** A shell over a small server: it answers the two reads, and applies (or refuses) a command as the DataGuard would. */
function fakeShell(options: { sessions?: unknown; refuse?: string[]; later?: number } = {}) {
  const sessions = structuredClone(options.sessions ?? SESSIONS) as Row[];
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables: unknown }> = [];
  let asked = 0;
  let applied = false;
  const apply = (command: Sent): { status: string; violations: string[] } => {
    if (options.refuse) return { status: 'rejected', violations: options.refuse };
    if (!applied) {
      applied = true;
      const order = sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1;
      sessions.push({ id: `s${order}`, order, title: command.payload.title, date: command.payload.plannedOn, status: 'prep', present: command.payload.present, summary: null });
    }
    return { status: 'applied', violations: [] };
  };
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      return { asOf: 1, data: id === 'session.listSessions' ? structuredClone(sessions) : structuredClone(PEOPLE) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = options.later === undefined ? apply(command) : { status: 'queued', violations: [] };
      return { commandId: 'c1', result: { status: outcome.status, violations: outcome.violations } };
    },
    async commandResult() {
      asked += 1;
      const outcome = apply(sent[0] as Sent);
      return { result: { commandId: 'c1', status: asked > (options.later ?? 0) ? outcome.status : 'queued', violations: outcome.violations } };
    },
  };
  return { shell, sent, reads };
}

const env: Env = { today: () => '2026-10-31', sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the list finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function list(options?: Parameters<typeof fakeShell>[0]) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="SessionList"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, { campaign: 'cendres-valombre' }, fake.shell, env);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is in the list`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const type = (selector: string, value: string): void => {
    ($(selector) as HTMLInputElement).value = value;
  };
  const tick = (id: string, checked: boolean): void => {
    (host.querySelector(`input[name="present"][value="${id}"]`) as HTMLInputElement).checked = checked;
  };
  return { host, $, click, type, tick, ...fake };
}

test('the sessions show in order, each with its summary and the PCs who were there', async () => {
  const shuffled = [...SESSIONS].reverse();
  const l = await list({ sessions: shuffled });
  const rows = [...l.host.querySelectorAll('#sessions-items > li')];
  assert.deepEqual(rows.map((r) => r.querySelector('.mf-sessions__num')?.textContent), ['1', '2', '3', '4']);
  assert.equal(rows[0]?.querySelector('h3')?.textContent, 'Arrivée à Valombre');
  // previous-summary-visible: a closed session's summary is in the list.
  assert.match(rows[0]?.querySelector('.mf-sessions__summary')?.textContent ?? '', /pluie de cendres/);
  assert.equal(rows[1]?.querySelector('[data-reads="campaign.listCharacters"]')?.textContent, 'PJ présents : Elowen, Brannoc, Kael');
  assert.match(rows[3]?.className ?? '', /is-next/);
  assert.deepEqual(l.reads.map((r) => r.id).sort(), ['campaign.listCharacters', 'session.listSessions']);
  assert.deepEqual(l.reads[0]?.variables, { campaign: 'cendres-valombre' });
});

test('a session of the real Capability — closed, with a label, no summary yet — shows as played', async () => {
  const l = await list({ sessions: [{ id: 'a', order: 1, title: 'Un', date: null, status: 'closed', label: 'Jouée', present: ['elowen'], summary: null }] });
  assert.match(l.$('#session-a').textContent ?? '', /Jouée/);
  assert.equal(l.host.querySelector('.mf-sessions__summary'), null);
  assert.doesNotMatch(l.$('#session-a').className, /is-next/);
});

test('the creation form offers only the PCs (present-only-pj), all ticked, and a default title and date', async () => {
  const l = await list();
  await l.click('#sessions-new');
  const offered = [...l.host.querySelectorAll<HTMLInputElement>('input[name="present"]')];
  assert.deepEqual(offered.map((o) => o.value), PCS);
  assert.ok(PEOPLE.some((p) => p.kind === 'pnj'), 'the fixture holds NPCs the form must leave out');
  assert.ok(offered.every((o) => o.checked));
  assert.equal((l.$('#session-create-title') as HTMLInputElement).value, 'Séance 5');
  assert.equal((l.$('#session-create-date') as HTMLInputElement).value, '2026-10-31');
});

test('saving triggers session.createSession by identifier with the title, the date and the ticked PCs, and the session appears last', async () => {
  const l = await list();
  await l.click('#sessions-new');
  l.type('#session-create-title', '  Le gué  ');
  l.type('#session-create-date', '2026-11-14');
  l.tick('sirielle', false);
  l.tick('kael', false);
  await l.click('#session-create-submit');
  assert.equal(l.sent.length, 1);
  const { idempotencyKey, ...command } = l.sent[0] as Sent & { idempotencyKey: string };
  assert.ok(idempotencyKey.length > 0, 'a creation carries its idempotency key');
  assert.deepEqual(command, {
    dataCapability: 'session.createSession@1',
    payload: { campaign: 'cendres-valombre', title: 'Le gué', plannedOn: '2026-11-14', present: ['elowen', 'brannoc'] },
    caller: { by: 'dm', from: 'SessionList', roles: [] },
  });
  assert.equal(l.host.querySelector('#session-create-dialog'), null, 'the dialog closes once it is applied');
  const rows = [...l.host.querySelectorAll('#sessions-items > li')];
  assert.equal(rows.length, 5);
  assert.equal(rows[4]?.querySelector('h3')?.textContent, 'Le gué');
  assert.equal(rows[4]?.querySelector('[data-reads="campaign.listCharacters"]')?.textContent, 'PJ présents : Elowen, Brannoc');
});

test('a command the server applies later is followed until it settles', async () => {
  const l = await list({ later: 2 });
  await l.click('#sessions-new');
  await l.click('#session-create-submit');
  assert.equal(l.host.querySelectorAll('#sessions-items > li').length, 5);
});

test('a refused creation says why and keeps the dialog open; an empty title or date is not sent', async () => {
  const l = await list({ refuse: ['present-only-pj'] });
  await l.click('#sessions-new');
  l.type('#session-create-title', '   ');
  await l.click('#session-create-submit');
  assert.match(l.$('#session-create-error').textContent ?? '', /titre/);
  l.type('#session-create-title', 'Le gué');
  l.type('#session-create-date', '');
  await l.click('#session-create-submit');
  assert.match(l.$('#session-create-error').textContent ?? '', /date/);
  assert.equal(l.sent.length, 0);
  l.type('#session-create-date', '2026-11-14');
  await l.click('#session-create-submit');
  await l.click('#session-create-submit');
  assert.equal(l.sent.length, 2);
  assert.notEqual((l.sent[0] as Sent & { idempotencyKey: string }).idempotencyKey, (l.sent[1] as Sent & { idempotencyKey: string }).idempotencyKey, 'a refusal is final for its key');
  assert.match(l.$('#session-create-error').textContent ?? '', /seuls les PJ de la campagne/);
  assert.equal(l.host.querySelectorAll('#sessions-items > li').length, 4);
  await l.click('#session-create-cancel');
  assert.equal(l.host.querySelector('#session-create-dialog'), null);
});

test('a creation the server is slow to settle, sent again, is the same creation — and a changed one is not', async () => {
  const l = await list({ later: 1000 });
  await l.click('#sessions-new');
  await l.click('#session-create-submit');
  assert.match(l.$('#session-create-error').textContent ?? '', /n’a pas abouti/);
  await l.click('#session-create-submit');
  l.type('#session-create-title', 'Autre titre');
  await l.click('#session-create-submit');
  const keys = l.sent.map((c) => (c as Sent & { idempotencyKey: string }).idempotencyKey);
  assert.equal(keys.length, 3);
  assert.equal(keys[0], keys[1]);
  assert.notEqual(keys[1], keys[2]);
});

test('the form is not offered while the characters cannot be read: it would tick nobody', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const { shell } = fakeShell();
  const noCharacters: Shell = { ...shell, capability: (id, sha, variables) => (id === 'campaign.listCharacters' ? Promise.reject(new Error('down')) : shell.capability(id, sha, variables)) };
  mount(host, { campaign: 'c' }, noCharacters, env);
  await settle();
  host.querySelector('#sessions-new')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(host.querySelector('#session-create-dialog'), null);
  assert.match(host.querySelector('#sessions-status')?.textContent ?? '', /personnages/);
  assert.equal(host.querySelectorAll('#sessions-items > li').length, 4, 'the sessions still show');
});

test('mounting again on the same host does not stack a second set of listeners', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const fake = fakeShell();
  mount(host, { campaign: 'c' }, fake.shell, env);
  mount(host, { campaign: 'c' }, fake.shell, env);
  await settle();
  host.querySelector('#sessions-new')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(host.querySelectorAll('#session-create-dialog').length, 1);
  host.querySelector('#session-create-submit')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(fake.sent.length, 1);
});

test('an older read never overwrites a newer one', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const fake = fakeShell();
  let release: () => void = () => undefined;
  const slow = new Promise<void>((resolve) => {
    release = resolve;
  });
  let firstSessionsRead = true;
  const shell: Shell = {
    ...fake.shell,
    async capability(id, sha, variables) {
      // The first read of the sessions is answered late, with the list as it was before the creation.
      if (id === 'session.listSessions' && firstSessionsRead) {
        firstSessionsRead = false;
        const before = await fake.shell.capability(id, sha, variables);
        await slow;
        return before;
      }
      return fake.shell.capability(id, sha, variables);
    },
  };
  mount(host, { campaign: 'c' }, shell, env);
  await settle();
  host.querySelector('#sessions-new')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(host.querySelector('#session-create-dialog'), null, 'the dialog waits for the first read');
  release();
  await settle();
  host.querySelector('#sessions-new')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  host.querySelector('#session-create-submit')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(host.querySelectorAll('#sessions-items > li').length, 5);
});

test('a list that cannot be read says so, and the button is still there', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const down: Shell = { capability: () => Promise.reject(new Error('down')), command: () => Promise.reject(new Error('down')), commandResult: () => Promise.reject(new Error('down')) };
  mount(host, {}, down, env);
  await settle();
  assert.match(host.querySelector('#sessions-status')?.textContent ?? '', /Impossible de lire les séances/);
  assert.ok(host.querySelector('#sessions-new'));
});

test('its parts name what they show and trigger, by identifier (contract D)', async () => {
  const l = await list();
  assert.ok(l.host.querySelector('[data-reads="session.listSessions"]'));
  assert.ok(l.host.querySelector('[data-reads="campaign.listCharacters"]'));
  assert.equal(l.$('#sessions-new').dataset.writes, 'session.createSession');
  await l.click('#sessions-new');
  assert.equal(l.$('#session-create-submit').dataset.writes, 'session.createSession');
});
