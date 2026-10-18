import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const fixtureOf = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));
/** The mock-up's fixtures of the two Capabilities: what the list is opened on. */
const SESSIONS = fixtureOf('session.listSessions') as Array<{ id: string; title: string; status: string; summary: string; present: string[] }>;
const CHARACTERS = fixtureOf('campaign.listCharacters') as Array<{ id: string; name: string; kind: string }>;
const PCS = CHARACTERS.filter((c) => c.kind === 'pj');

interface Sent {
  dataCapability: string;
  payload: { campaign: string; title: string; plannedOn?: string; present: string[] };
  caller: { by: string };
  idempotencyKey: string;
}

interface Options {
  failSessions?: boolean;
  failCharacters?: boolean;
  /** What the server answers a command with. */
  answer?: (sent: Sent) => { status: string; violations?: string[] };
  later?: number;
  /** The server cannot be reached when a command is sent. */
  failCommand?: boolean;
}

/** A shell over a small server: it answers the two reads, and applies a command by adding the session at the end. */
function fakeShell(options: Options = {}) {
  const sessions = structuredClone(SESSIONS) as unknown[];
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables: unknown }> = [];
  let asked = 0;
  const answer: NonNullable<Options['answer']> = options.answer ?? (() => ({ status: 'applied' }));
  const apply = (command: Sent): { status: string; violations?: string[] } => {
    const outcome = answer(command);
    if (outcome.status === 'applied' && !sessions.some((s) => (s as { title: string }).title === command.payload.title)) {
      sessions.push({ id: `new-${sent.length}`, order: sessions.length + 1, title: command.payload.title, date: command.payload.plannedOn ?? null, status: 'prep', label: 'En préparation', present: command.payload.present, summary: null });
    }
    return outcome;
  };
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      if (id === 'session.listSessions') {
        if (options.failSessions) throw new Error('down');
        return { asOf: 1, data: structuredClone(sessions) };
      }
      if (options.failCharacters) throw new Error('down');
      return { asOf: 1, data: structuredClone(CHARACTERS) };
    },
    async command(body) {
      if (options.failCommand) throw new Error('down');
      const command = body as Sent;
      sent.push(command);
      const outcome = apply(command);
      return { commandId: `c${sent.length}`, result: { status: options.later === undefined ? outcome.status : 'queued', violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      asked += 1;
      const outcome = answer(sent[Number(commandId.slice(1)) - 1] as Sent);
      return { result: { commandId, status: asked > (options.later ?? 0) ? outcome.status : 'queued', violations: outcome.violations ?? [] } };
    },
  };
  return { shell, sent, reads };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the list finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function list(options?: Options, props: Record<string, unknown> = { campaign: 'cendres-valombre' }) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="SessionList"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the list`);
    return found;
  };
  const type = (selector: string, value: string): void => {
    const field = $(selector) as HTMLInputElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  const tick = (id: string, checked: boolean): void => {
    const box = $(`input[name="present"][value="${id}"]`) as HTMLInputElement;
    box.checked = checked;
    box.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const ids = (): string[] => [...host.querySelectorAll('#sessions-items > li')].map((li) => li.id);
  return { host, document, window: dom.window, mounted, $, type, tick, click, ids, text: (): string => host.textContent ?? '', ...fake };
}

test('the sessions are listed in order, each with its summary and the first names of the PCs present', async () => {
  const l = await list();
  assert.deepEqual(l.ids(), SESSIONS.map((s) => `session-${s.id}`));
  for (const s of SESSIONS) {
    const row = l.$(`#session-${s.id}`);
    assert.match(row.textContent ?? '', new RegExp(s.title));
    assert.ok((row.textContent ?? '').includes(s.summary), `${s.id} shows its summary`);
    const names = s.present.map((id) => (CHARACTERS.find((c) => c.id === id) as { name: string }).name.split(' ')[0]).join(', ');
    assert.ok((row.textContent ?? '').includes(`PJ présents : ${names}`), `${s.id} shows who was present`);
  }
  assert.deepEqual(l.reads, [
    { id: 'session.listSessions', variables: { campaign: 'cendres-valombre' } },
    { id: 'campaign.listCharacters', variables: { campaign: 'cendres-valombre' } },
  ]);
});

test('a closed session shows its summary, the session in preparation is the one to draw attention to', async () => {
  const l = await list();
  const closed = SESSIONS.filter((s) => s.status === 'played');
  assert.ok(closed.length > 0);
  for (const s of closed) {
    assert.ok((l.$(`#session-${s.id}`).textContent ?? '').includes(s.summary));
    assert.equal(l.$(`#session-${s.id}`).classList.contains('is-next'), false);
  }
  const next = SESSIONS.find((s) => s.status === 'prep');
  assert.ok(next);
  assert.equal(l.$(`#session-${next.id}`).classList.contains('is-next'), true);
});

test('the parts name what they show and what they trigger, by identifier', async () => {
  const l = await list();
  assert.equal(l.$('#sessions-items').dataset.reads, 'session.listSessions');
  assert.ok(l.host.querySelector('[data-reads="campaign.listCharacters"]'));
  assert.equal(l.$('#sessions-new').dataset.writes, 'session.createSession');
  await l.click('#sessions-new');
  assert.equal(l.$('#session-create-submit').dataset.writes, 'session.createSession');
  assert.equal(l.$('#session-create-present').dataset.reads, 'campaign.listCharacters');
});

test('the creation form offers the PCs only, each ticked at first (present-only-pj)', async () => {
  const l = await list();
  assert.equal(l.host.querySelector('#session-create-dialog'), null);
  await l.click('#sessions-new');
  const offered = [...l.host.querySelectorAll<HTMLInputElement>('input[name="present"]')];
  assert.deepEqual(offered.map((box) => box.value), PCS.map((c) => c.id));
  assert.ok(CHARACTERS.length > PCS.length, 'the fixture has NPCs to leave out');
  assert.ok(offered.every((box) => box.checked));
  assert.equal((l.$('#session-create-title') as HTMLInputElement).value, `Séance ${SESSIONS.length + 1}`);
});

test('a title, a date and two PCs ticked: session.createSession is triggered with them, and the session appears last', async () => {
  const l = await list();
  await l.click('#sessions-new');
  l.type('#session-create-title', 'Le gué des Corbeaux');
  l.type('#session-create-date', '2026-10-31');
  for (const pc of PCS.slice(2)) l.tick(pc.id, false);
  await l.click('#session-create-submit');
  assert.equal(l.sent.length, 1);
  const { idempotencyKey, ...command } = l.sent[0] as Sent;
  assert.ok(idempotencyKey.length > 0);
  assert.deepEqual(command, {
    dataCapability: 'session.createSession@1',
    payload: { campaign: 'cendres-valombre', title: 'Le gué des Corbeaux', plannedOn: '2026-10-31', present: PCS.slice(0, 2).map((c) => c.id) },
    caller: { by: 'dm', from: 'SessionList', roles: [] },
  });
  assert.equal(l.host.querySelector('#session-create-dialog'), null, 'the form closes once the session is created');
  const ids = l.ids();
  assert.equal(ids.length, SESSIONS.length + 1);
  assert.ok((l.$(`#${ids[ids.length - 1]}`).textContent ?? '').includes('Le gué des Corbeaux'));
});

test('a command refused keeps the form open with its reason, and the list as it was', async () => {
  const l = await list({ answer: () => ({ status: 'rejected', violations: ['present-only-pj'] }) });
  await l.click('#sessions-new');
  await l.click('#session-create-submit');
  assert.match(l.$('#session-create-message').textContent ?? '', /^Création refusée : seuls les PJ de la campagne peuvent être présents\.$/);
  assert.equal(l.ids().length, SESSIONS.length);
  assert.equal((l.$('#session-create-submit') as HTMLButtonElement).disabled, false, 'the DM can try again');
});

test('a command that settles later is followed until it is applied', async () => {
  const l = await list({ later: 2 });
  await l.click('#sessions-new');
  await l.click('#session-create-submit');
  assert.equal(l.ids().length, SESSIONS.length + 1);
});

test('a command that never settles says so, and one that throws says it failed', async () => {
  const stuck = await list({ later: 99 });
  await stuck.click('#sessions-new');
  await stuck.click('#session-create-submit');
  assert.match(stuck.$('#session-create-message').textContent ?? '', /n’a pas abouti \(état : queued\)/);
  const down = await list({ failCommand: true });
  await down.click('#sessions-new');
  await down.click('#session-create-submit');
  assert.equal(down.$('#session-create-message').textContent, 'La création a échoué.');
  assert.equal(down.ids().length, SESSIONS.length);
});

test('cancelling, closing or pressing Escape sends nothing', async () => {
  const l = await list();
  for (const close of ['#session-create-cancel', '.kit-dialog__close']) {
    await l.click('#sessions-new');
    await l.click(close);
    assert.equal(l.host.querySelector('#session-create-dialog'), null);
  }
  await l.click('#sessions-new');
  l.$('#session-create-title').dispatchEvent(new l.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(l.host.querySelector('#session-create-dialog'), null);
  assert.equal(l.sent.length, 0);
});

test('sessions that cannot be read are said so; characters that cannot be are not needed to list them, only to create', async () => {
  const noSessions = await list({ failSessions: true });
  assert.match(noSessions.$('#sessions-items').textContent ?? '', /Impossible de lire les séances/);
  assert.equal((noSessions.$('#sessions-new') as HTMLButtonElement).disabled, true);
  const noCharacters = await list({ failCharacters: true });
  assert.equal(noCharacters.ids().length, SESSIONS.length);
  assert.match(noCharacters.$('#session-s2').textContent ?? '', /PJ présents : elowen, brannoc, kael/);
  assert.equal((noCharacters.$('#sessions-new') as HTMLButtonElement).disabled, true);
  assert.match(noCharacters.$('#sessions-new').title, /Impossible de lire les PJ/);
});

test('the form cannot be opened before the reads answer, so it never offers a roster still to come', async () => {
  const dom = new JSDOM('<!doctype html><div></div>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (release = resolve));
  mount(host, {}, { capability: async (id) => { await gate; return { asOf: 1, data: id === 'session.listSessions' ? SESSIONS : CHARACTERS }; }, command: async () => ({}), commandResult: async () => ({}) }, timing);
  assert.equal((host.querySelector('#sessions-new') as HTMLButtonElement).disabled, true);
  release();
  await settle();
  assert.equal((host.querySelector('#sessions-new') as HTMLButtonElement).disabled, false);
});

test('a creation tried again after it did not settle is the same command; an edit or a refusal makes another', async () => {
  const stuck = await list({ later: 99 });
  await stuck.click('#sessions-new');
  await stuck.click('#session-create-submit');
  await stuck.click('#session-create-submit');
  assert.equal(stuck.sent.length, 2);
  assert.equal(stuck.sent[0]?.idempotencyKey, stuck.sent[1]?.idempotencyKey, 'the same form sent twice is one command');
  stuck.type('#session-create-title', 'Autre titre');
  await stuck.click('#session-create-submit');
  assert.notEqual(stuck.sent[2]?.idempotencyKey, stuck.sent[1]?.idempotencyKey, 'an edited form is another command');
  const refused = await list({ answer: () => ({ status: 'rejected', violations: ['present-only-pj'] }) });
  await refused.click('#sessions-new');
  await refused.click('#session-create-submit');
  await refused.click('#session-create-submit');
  assert.notEqual(refused.sent[0]?.idempotencyKey, refused.sent[1]?.idempotencyKey, 'after a refusal the next try is not its replay');
});

test('a read that fails after a creation leaves the sessions on screen', async () => {
  const l = await list();
  await l.click('#sessions-new');
  l.reads.length = 0;
  // From now on the sessions cannot be read.
  const read = l.shell.capability.bind(l.shell);
  l.shell.capability = async (id, sha, variables) => {
    if (id === 'session.listSessions') throw new Error('down');
    return read(id, sha, variables);
  };
  await l.click('#session-create-submit');
  assert.equal(l.ids().length, SESSIONS.length, 'the list is still there');
  assert.equal(l.host.querySelector('#session-create-dialog'), null, 'the session was created all the same');
});

test('a session being played is the one to resume, not to prepare', async () => {
  const live = JSON.stringify([{ id: 'l1', order: 1, title: 'En cours', date: '2026-10-17', status: 'live', label: 'En direct', present: [], summary: null }]);
  const dom = new JSDOM('<!doctype html><div></div>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  mount(host, {}, { capability: async (id) => ({ asOf: 1, data: id === 'session.listSessions' ? JSON.parse(live) : [] }), command: async () => ({}), commandResult: async () => ({}) }, timing);
  await settle();
  const row = host.querySelector('#session-l1') as HTMLElement;
  assert.equal(row.classList.contains('is-next'), true);
  assert.deepEqual([...row.querySelectorAll('button')].map((b) => b.textContent), ['Reprendre en direct']);
  assert.match(row.textContent ?? '', /En direct/);
});

test('while the reads are out, the list says it loads rather than that it is empty', async () => {
  const dom = new JSDOM('<!doctype html><div></div>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (release = resolve));
  mount(host, {}, { capability: async () => { await gate; return { asOf: 1, data: [] }; }, command: async () => ({}), commandResult: async () => ({}) }, timing);
  assert.match(host.querySelector('#sessions-items')?.textContent ?? '', /Chargement des séances/);
  release();
  await settle();
  assert.match(host.querySelector('#sessions-items')?.textContent ?? '', /Aucune séance/);
});

test('another campaign keeps nothing of the last one: its PCs’ names are not lent to the new sessions', async () => {
  const dom = new JSDOM('<!doctype html><div></div>');
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const shell: Shell = {
    async capability(id, _sha, variables) {
      if (id === 'campaign.listCharacters') {
        if (variables?.campaign === 'c2') throw new Error('down');
        return { asOf: 1, data: CHARACTERS };
      }
      return { asOf: 1, data: SESSIONS };
    },
    command: async () => ({}),
    commandResult: async () => ({}),
  };
  const mounted = mount(host, { campaign: 'c1' }, shell, timing);
  await settle();
  assert.match(host.querySelector('#session-s2')?.textContent ?? '', /PJ présents : Elowen, Brannoc, Kael/);
  mounted.update({ campaign: 'c2' });
  await settle();
  assert.match(host.querySelector('#session-s2')?.textContent ?? '', /PJ présents : elowen, brannoc, kael/);
  assert.equal((host.querySelector('#sessions-new') as HTMLButtonElement).disabled, true);
});

test('a campaign with no session yet says so, and follows its campaign prop', async () => {
  const l = await list(undefined, { campaign: 'c1' });
  await l.click('#sessions-new');
  l.mounted.update({ campaign: 'c2' });
  assert.equal(l.host.querySelector('#session-create-dialog'), null, 'a form half filled with the last campaign’s PCs is gone');
  assert.match(l.$('#sessions-items').textContent ?? '', /Chargement/, 'so are its sessions');
  await settle();
  assert.deepEqual(l.reads.filter((r) => r.id === 'session.listSessions').map((r) => r.variables), [{ campaign: 'c1' }, { campaign: 'c2' }]);
  const empty = new JSDOM('<!doctype html><div></div>');
  const host = empty.window.document.querySelector('div') as HTMLElement;
  mount(host, {}, { capability: async () => ({ asOf: 1, data: [] }), command: async () => ({}), commandResult: async () => ({}) }, timing);
  await settle();
  assert.match(host.querySelector('#sessions-items')?.textContent ?? '', /Aucune séance/);
});
