import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const fixtureOf = (id: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${id}.json`, import.meta.url), 'utf8'));

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  target: { aggregate: string; id: string };
  idempotencyKey?: string;
}

/** A shell over the mock-up's fixtures: it answers the seven Capabilities and applies a command as asked. */
function fakeShell(options: { apply?: (sent: Sent) => { status: string; violations?: string[] }; fail?: string; scenes?: unknown; sessionCampaign?: string; present?: string[]; noSessionId?: boolean; context?: string } = {}) {
  const data: Record<string, unknown> = {
    'session.getSession': { ...(fixtureOf('session.getSession') as object), ...(options.present ? { present: options.present } : {}), ...(options.noSessionId ? { id: '' } : {}), ...(options.sessionCampaign ? { campaign: options.sessionCampaign } : {}) },
    'session.listScenes': options.scenes ?? fixtureOf('session.listScenes'),
    'campaign.listCharacters': fixtureOf('campaign.listCharacters'),
    'campaign.listPlaces': fixtureOf('campaign.listPlaces'),
    'campaign.listEvents': fixtureOf('campaign.listEvents'),
    'media.listMedia': fixtureOf('media.listMedia'),
    'table.getBattlemapInstance': fixtureOf('table.getBattlemapInstance'),
  };
  const sent: Sent[] = [];
  const reads: Array<{ id: string; variables?: Record<string, unknown> }> = [];
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push({ id, variables });
      if (id === options.fail) throw new Error('down');
      return { asOf: 1, data: structuredClone(data[id]) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = options.apply?.(command) ?? { status: 'applied' };
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult() {
      const outcome = options.apply?.(sent.at(-1) as Sent) ?? { status: 'applied' };
      return { result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    context: { get: (name) => (name === 'session' ? ('context' in options ? options.context : 's4') : undefined) },
  };
  return { shell, sent, reads, data };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function board(options?: Parameters<typeof fakeShell>[0], props: Record<string, unknown> = {}) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="SceneBoard"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the board`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  return { host, $, click, dom, ...fake };
}

test('the board shows the present PCs, the scenes in order, and each scene\'s battlemap', async () => {
  const b = await board();
  assert.match(b.$('#scenes-title').textContent ?? '', /séance 4 — L’embuscade du Gué/);
  assert.equal(b.$('#scenes-present-count').textContent, '4 sur 4');
  assert.equal(b.$('#scenes-present-list').children.length, 4);
  assert.deepEqual([...b.host.querySelectorAll('.mf-scenes__scene h3')].map((h) => h.textContent), ['Retour à la Taverne du Sanglier', 'La route des Corbeaux', 'L’embuscade au Gué']);
  assert.match(b.$('#sc3-battlemap').textContent ?? '', /Le Gué des Corbeaux/);
  assert.equal(b.$('#sc3-battlemap').dataset.reads, 'table.getBattlemapInstance');
  assert.deepEqual(b.reads.filter((r) => r.id === 'table.getBattlemapInstance').map((r) => r.variables), [{ instance: 'bmi-gue-s4', view: 'dm' }]);
  assert.deepEqual(b.reads[0]?.variables, { id: 's4' }, 'session.getSession is asked by id');
  assert.deepEqual(b.reads.find((r) => r.id === 'session.listScenes')?.variables, { session: 's4' });
});

test('a scene with an instance offers no second one; a scene without offers the campaign\'s battlemaps', async () => {
  const b = await board();
  assert.equal(b.host.querySelector('#sc3-battlemap [data-writes]'), null);
  assert.equal(b.host.querySelector('#sc3-battlemap select'), null);
  const options = [...b.$('#sc1-battlemap select').querySelectorAll('option')].map((o) => o.textContent);
  assert.deepEqual(options, ['Le Gué des Corbeaux', 'Les ruines du moulin']);
  assert.equal(b.$('#sc1-battlemap [data-writes]').dataset.writes, 'table.createBattlemapInstance');
});

test('ticking a PC triggers session.addPresentCharacter, and unticking triggers session.removePresentCharacter', async () => {
  const b = await board();
  await b.click('[data-remove-pj="kael"]');
  assert.deepEqual(b.sent.map((s) => [s.dataCapability, s.payload]), [['session.removePresentCharacter@1', { session: 's4', character: 'kael' }]]);
  assert.deepEqual(b.sent[0]?.target, { aggregate: 'Session', id: 's4' });

  (b.data['session.getSession'] as { present: string[] }).present = ['elowen', 'brannoc', 'sirielle'];
  await b.click('[data-remove-pj="sirielle"]');
  (b.$('#scenes-present-select') as HTMLSelectElement).value = 'kael';
  await b.click('#scenes-present-submit');
  assert.equal(b.sent.at(-1)?.dataCapability, 'session.addPresentCharacter@1');
  assert.deepEqual(b.sent.at(-1)?.payload, { session: 's4', character: 'kael' });
});

test('present-only-pj: only the PCs of the campaign are offered, never an NPC', async () => {
  const b = await board({ present: [] });
  const offered = [...b.$('#scenes-present-select').querySelectorAll('option')].map((o) => o.value);
  assert.deepEqual(offered, ['elowen', 'brannoc', 'sirielle', 'kael']);
});

test('moving the third scene first triggers session.reorderScenes with every rank rewritten', async () => {
  const b = await board();
  await b.click('#scene-sc3 [data-move="-1"]');
  await b.click('#scene-sc2 [data-move="-1"]');
  assert.equal(b.sent.length, 2);
  assert.equal(b.sent[0]?.dataCapability, 'session.reorderScenes@1');
  assert.deepEqual(b.sent[0]?.payload.order, [{ scene: 'sc1', rank: 1 }, { scene: 'sc3', rank: 2 }, { scene: 'sc2', rank: 3 }]);
});

test('the first scene cannot go up and the last cannot go down', async () => {
  const b = await board();
  assert.equal((b.$('#scene-sc1 [data-move="-1"]') as HTMLButtonElement).disabled, true);
  assert.equal((b.$('#scene-sc3 [data-move="1"]') as HTMLButtonElement).disabled, true);
  await b.click('#scene-sc1 [data-move="-1"]');
  assert.equal(b.sent.length, 0);
});

test('picking a battlemap for a scene triggers table.createBattlemapInstance, once', async () => {
  const b = await board();
  (b.$('#sc1-battlemap select') as HTMLSelectElement).value = 'bm-moulin';
  const scenes = b.data['session.listScenes'] as Array<{ id: string; battlemap: string | null }>;
  await b.click('#sc1-battlemap [data-writes]');
  assert.deepEqual(b.sent.map((s) => [s.dataCapability, s.payload]), [['table.createBattlemapInstance@1', { session: 's4', scene: 'sc1', battlemap: 'bm-moulin' }]]);
  assert.match(b.sent[0]?.idempotencyKey ?? '', /^table\.createBattlemapInstance:sc1:\d+$/, 'one key per click, so a retry is a new command');
  // the server now gives the scene its instance: the board offers no second one
  scenes[0]!.battlemap = 'bmi-moulin-sc1';
  await b.click('#scenes-add');
  assert.equal(b.host.querySelector('#sc1-battlemap select'), null);
  assert.equal(b.host.querySelector('#sc1-battlemap [data-writes]'), null);
});

test('« Ajouter une scène » triggers session.addScene', async () => {
  const b = await board();
  await b.click('#scenes-add');
  assert.equal(b.sent[0]?.dataCapability, 'session.addScene@1');
  assert.equal(b.sent[0]?.payload.session, 's4');
});

test('a refused command says why and leaves the board as it was', async () => {
  const b = await board({ apply: () => ({ status: 'rejected', violations: ['present-only-pj'] }) });
  await b.click('[data-remove-pj="kael"]');
  assert.match(b.$('#scenes-notice').textContent ?? '', /seul un PJ de la campagne peut être présent/);
  assert.equal(b.$('#scenes-present-list').children.length, 4);
  assert.equal(b.reads.filter((r) => r.id === 'session.getSession').length, 1, 'a refused command reads nothing again');
});

test('after an applied command the board reads again, so it shows what the server holds', async () => {
  const b = await board();
  (b.data['session.getSession'] as { present: string[] }).present = ['elowen'];
  await b.click('[data-remove-pj="kael"]');
  assert.equal(b.$('#scenes-present-count').textContent, '1 sur 4');
});

test('a scene whose instance cannot be read still shows it, without its figures', async () => {
  const b = await board({ fail: 'table.getBattlemapInstance' });
  assert.match(b.$('#sc3-battlemap').textContent ?? '', /bmi-gue-s4/);
  assert.equal(b.host.querySelector('#sc3-battlemap select'), null);
});

test('when the session cannot be read the board says so and keeps its titles', async () => {
  const b = await board({ fail: 'session.getSession' });
  assert.match(b.$('#scenes-notice').textContent ?? '', /Impossible de lire la séance/);
  assert.equal(b.$('#scenes-title').textContent, 'Scènes');
});

test('every part names the unit it shows or triggers by identifier', async () => {
  const b = await board({ present: ['elowen'] });
  const writes = new Set([...b.host.querySelectorAll('[data-writes]')].map((e) => (e as HTMLElement).dataset.writes));
  for (const id of ['session.addPresentCharacter', 'session.removePresentCharacter', 'session.addScene', 'session.reorderScenes', 'table.createBattlemapInstance']) assert.ok(writes.has(id), `${id} has an actuator`);
  const reads = (b.host.innerHTML.match(/data-reads="([^"]+)"/g) ?? []).join(' ');
  for (const id of ['session.getSession', 'session.listScenes', 'campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents', 'media.listMedia', 'table.getBattlemapInstance']) assert.match(reads, new RegExp(id.replace('.', '\\.')), `${id} has a part`);
});

test('a session the board does not know is one it writes nothing to', async () => {
  const b = await board({ present: ['elowen'], noSessionId: true });
  await b.click('#scenes-add');
  assert.equal(b.sent.length, 0);
  assert.match(b.$('#scenes-notice').textContent ?? '', /pas identifiée/);
});

test('a command that is not applied in time is said so, and the board shows what the server holds', async () => {
  const b = await board({ apply: () => ({ status: 'queued' }), present: ['elowen'] });
  await b.click('[data-remove-pj="elowen"]');
  assert.match(b.$('#scenes-notice').textContent ?? '', /pas encore appliquée \(état : queued\)/);
  assert.equal(b.reads.filter((r) => r.id === 'session.getSession').length, 2);
});

test('an entity link carries its id encoded, so a name with « & » or a space still opens that entity', async () => {
  const scenes = (fixtureOf('session.listScenes') as Array<Record<string, unknown>>).map((scene) => ({ ...scene, places: ['la cave & le puits'] }));
  const b = await board({ scenes });
  assert.equal(b.$('#sc1-place-la\\ cave\\ \\&\\ le\\ puits').getAttribute('href'), 'fiche-entite.html?id=la%20cave%20%26%20le%20puits');
});

test('the session prop is what session.getSession is asked for by id, over the screen\'s context value', async () => {
  const b = await board({ context: 's3' }, { session: 's4' });
  assert.deepEqual(b.reads.find((r) => r.id === 'session.getSession')?.variables, { id: 's4' });
  assert.deepEqual(b.reads.find((r) => r.id === 'session.listScenes')?.variables, { session: 's4' });
  assert.equal(b.host.querySelectorAll('.mf-scenes__scene').length, 3);
});

test('with the session prop, the commands carry that session', async () => {
  const b = await board({ context: undefined }, { session: 's4' });
  await b.click('#scenes-add');
  await b.click('#scene-sc3 [data-move="-1"]');
  assert.deepEqual(b.sent.map((s) => [s.dataCapability, s.payload.session, s.target]), [
    ['session.addScene@1', 's4', { aggregate: 'Session', id: 's4' }],
    ['session.reorderScenes@1', 's4', { aggregate: 'Session', id: 's4' }],
  ]);
});

test('with no prop and no context value the board reads nothing, shows no scene and sends no write', async () => {
  const b = await board({ context: undefined });
  assert.deepEqual(b.reads, []);
  assert.equal(b.host.querySelectorAll('.mf-scenes__scene').length, 0);
  assert.match(b.$('#scenes-notice').textContent ?? '', /Aucune séance/);
  await b.click('#scenes-add');
  assert.equal(b.sent.length, 0);
});

const campaignReads = ['campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents', 'media.listMedia'];
const readsOf = (b: { reads: Array<{ id: string; variables?: Record<string, unknown> }> }, ids: string[]) => b.reads.filter((r) => ids.includes(r.id)).map((r) => [r.id, r.variables]);

test('a scene shows the names of its places and events, not their ids', async () => {
  const b = await board();
  assert.equal(b.$('#sc1-place-taverne-sanglier').textContent, 'La Taverne du Sanglier');
  assert.equal(b.$('#sc1-event-marche-noir').textContent, 'Le marché noir de Morlox');
  // The link still opens the entity by its id.
  assert.equal(b.$('#sc1-place-taverne-sanglier').getAttribute('href'), 'fiche-entite.html?id=taverne-sanglier');
});

test('a place or an event the campaign does not list stays its id', async () => {
  const scenes = (fixtureOf('session.listScenes') as Array<Record<string, unknown>>).map((scene) => ({ ...scene, places: ['lieu-inconnu'], events: ['evt-inconnu'] }));
  const b = await board({ scenes });
  assert.equal(b.$('#sc1-place-lieu-inconnu').textContent, 'lieu-inconnu');
  assert.equal(b.$('#sc1-event-evt-inconnu').textContent, 'evt-inconnu');
});

test('session.getSession gives no campaign: the campaign prop is what the four campaign reads are for', async () => {
  const b = await board({}, { session: 's4', campaign: 'c1' });
  assert.deepEqual(readsOf(b, campaignReads).sort(), campaignReads.map((id) => [id, { campaign: 'c1' }]).sort());
  assert.equal(b.$('#sc1-char-gaspard').textContent, 'Gaspard Grosmollet');
  assert.equal(b.$('#sc1-media-ill-gaspard').textContent, 'Gaspard, le tavernier');
});

test('the campaign session.getSession gives wins over the campaign prop', async () => {
  const b = await board({ sessionCampaign: 'c-session' }, { session: 's4', campaign: 'c-prop' });
  assert.deepEqual(readsOf(b, campaignReads).map(([, variables]) => variables), campaignReads.map(() => ({ campaign: 'c-session' })));
});

test('with no campaign from the session nor the prop, the campaign reads are asked with none', async () => {
  const b = await board({}, { session: 's4', campaign: '' });
  assert.deepEqual(readsOf(b, campaignReads).map(([, variables]) => variables), campaignReads.map(() => ({})));
});

test('the campaign reads fail with the board: it says so and keeps its titles', async () => {
  const b = await board({ fail: 'campaign.listPlaces' }, { session: 's4', campaign: 'c1' });
  assert.match(b.$('#scenes-notice').textContent ?? '', /Impossible de lire la séance/);
});
