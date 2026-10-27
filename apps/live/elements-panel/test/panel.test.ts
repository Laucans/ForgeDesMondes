import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import { STYLE } from '../src/style.ts';
import type { Shell } from '../src/types.ts';

const mockFixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../../../mock/fixtures/${name}.json`, import.meta.url), 'utf8'));
const manifest = JSON.parse(readFileSync(new URL('../micro-ui.json', import.meta.url), 'utf8')) as { needs: string[]; actions: string[]; props: object };

const ANSWERS: Record<string, unknown> = Object.fromEntries(['session.listScenes', 'campaign.listCharacters', 'campaign.listPlaces', 'campaign.listEvents', 'media.listMedia'].map((id) => [id, mockFixture(id)]));

interface Sent {
  dataCapability: string;
  target: { aggregate: string; id?: string };
  payload: Record<string, unknown>;
  caller: { from: string };
}

interface Options {
  context?: Record<string, string>;
  /** The ids of the Capabilities whose read throws. */
  failing?: string[];
  /** What the server answers each command, in order; the last answer repeats. */
  status?: string | string[];
  violations?: string[];
  throwOnCommand?: boolean;
}

function fakeShell(options: Options = {}) {
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables?: Record<string, unknown> }> = [];
  const values = new Map(Object.entries(options.context ?? {}));
  const followers = new Set<(name: string, value: unknown) => void>();
  /** Where a command stands after it has been asked about `asked` times: the server settles it step by step. */
  const statusOf = (asked: number): string => {
    const steps = Array.isArray(options.status) ? options.status : [options.status ?? 'applied'];
    return (steps[asked] ?? steps.at(-1)) ?? 'applied';
  };
  const asks = new Map<string, number>();
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      if (options.failing?.includes(id)) throw new Error('the server is not there');
      return { asOf: 1, data: structuredClone(ANSWERS[id]) };
    },
    async command(body) {
      if (options.throwOnCommand) throw new Error('the server is not there');
      sent.push(body as Sent);
      return { commandId: `c${sent.length}`, result: { status: statusOf(0), violations: options.violations ?? [] } };
    },
    async commandResult(commandId) {
      const asked = (asks.get(commandId) ?? 0) + 1;
      asks.set(commandId, asked);
      return { result: { commandId, status: statusOf(asked), violations: options.violations ?? [] } };
    },
    context: {
      get: (name) => values.get(name),
      on(listener) {
        followers.add(listener);
        return () => followers.delete(listener);
      },
    },
  };
  const setContext = (name: string, value: string): void => {
    values.set(name, value);
    followers.forEach((f) => f(name, value));
  };
  return { shell, sent, reads, setContext };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 3 };

const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

const CONTEXT = { session: 's4', campaign: 'cendres-valombre' };

async function panel(options?: Options) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="ElementsPanel"></div></body></html>', { url: 'http://localhost/' });
  const doc = dom.window.document;
  const host = doc.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const heard: Array<{ name: string; detail: Record<string, unknown> }> = [];
  for (const name of ['fdm:open', 'fdm:open-map']) doc.addEventListener(name, (e) => heard.push({ name, detail: (e as CustomEvent<Record<string, unknown>>).detail }));
  mount(host, {}, fake.shell, timing);
  await settle();
  const $ = <T extends HTMLElement = HTMLElement>(selector: string): T => {
    const found = host.querySelector<T>(selector);
    assert.ok(found, `${selector} is in the panel`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true, cancelable: true }));
    await settle();
  };
  const type = async (value: string): Promise<void> => {
    const input = $<HTMLInputElement>('#elements-search-input');
    input.value = value;
    input.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    await settle();
  };
  const found = (): string[] => [...host.querySelectorAll('#elements-results-list .mf-ep__open')].map((b) => b.textContent ?? '');
  return { dom, doc, host, fake, heard, $, click, type, found };
}

test('the session comes first, one details per scene in the order given, then the campaign by type', async () => {
  const p = await panel({ context: CONTEXT });
  const titles = [...p.host.querySelectorAll('h2')].map((h) => h.textContent);
  assert.deepEqual(titles.slice(-2), ['Séance en cours', 'Toute la campagne']);
  const scenes = [...p.host.querySelectorAll<HTMLDetailsElement>('#elements-scenes details[data-scene-index]')];
  assert.deepEqual(scenes.map((d) => d.dataset.sceneIndex), ['0', '1', '2']);
  assert.deepEqual(scenes.map((d) => d.id), ['elements-scene-sc1', 'elements-scene-sc2', 'elements-scene-sc3']);
  assert.ok(scenes[0]?.open && scenes[0].classList.contains('is-current'));
  assert.ok(scenes.every((d) => d.open), 'every scene is open: any element is reachable at once');
  assert.deepEqual(scenes.map((d) => d.classList.contains('is-current')), [true, false, false]);
  const groups = [...p.host.querySelectorAll('#elements-groups > details > summary')].map((s) => s.textContent?.replace(/\s+/g, ' ').trim());
  assert.deepEqual(groups, ['PJ 4', 'PNJ 5', 'Lieux 4', 'Événements 3', 'Médias 7']);
  assert.ok(p.$('#elements-session').compareDocumentPosition(p.$('#elements-campaign')) & 4, 'the session region precedes the campaign');
});

test('it reads the five Capabilities by identifier alone, for the session and campaign of the screen', async () => {
  const p = await panel({ context: CONTEXT });
  assert.deepEqual(p.fake.reads.map((r) => r.id).sort(), [...manifest.needs].sort());
  assert.ok(p.fake.reads.every((r) => r.sha256 === undefined));
  assert.deepEqual(p.fake.reads.find((r) => r.id === 'session.listScenes')?.variables, { session: 's4' });
  for (const id of manifest.needs.filter((n) => n !== 'session.listScenes')) assert.deepEqual(p.fake.reads.find((r) => r.id === id)?.variables, { campaign: 'cendres-valombre' });
});

test('what the screen has not named is not asked: no session, no scenes read; no campaign, nothing else', async () => {
  const none = await panel();
  assert.deepEqual(none.fake.reads, []);
  assert.match(none.$('#elements-scenes').textContent ?? '', /Aucune scène/);
  assert.equal(none.$('#elements-status').hidden, true);

  const campaignOnly = await panel({ context: { campaign: 'c' } });
  assert.deepEqual(campaignOnly.fake.reads.map((r) => r.id).sort(), ['campaign.listCharacters', 'campaign.listEvents', 'campaign.listPlaces', 'media.listMedia']);
  assert.match(campaignOnly.$('#elements-scenes').textContent ?? '', /Aucune scène/);
});

test('the screen setting the session later makes the panel read again', async () => {
  const p = await panel();
  p.fake.setContext('session', 's4');
  await settle();
  assert.deepEqual(p.fake.reads.map((r) => [r.id, r.variables]), [['session.listScenes', { session: 's4' }]]);
  assert.equal(p.host.querySelectorAll('#elements-scenes details').length, 3);
  p.fake.setContext('unrelated', 'x');
  await settle();
  assert.equal(p.fake.reads.length, 1);
  p.fake.setContext('campaign', 'cendres-valombre');
  await settle();
  assert.equal(p.fake.reads.length, 6);
  assert.equal(p.host.querySelectorAll('#elements-groups details').length, 5);
});

test('a search in progress follows what is read again', async () => {
  const p = await panel({ context: CONTEXT });
  await p.type('moulin');
  assert.ok(p.found().length > 0);
  ANSWERS['campaign.listPlaces'] = [];
  ANSWERS['campaign.listEvents'] = [];
  ANSWERS['media.listMedia'] = { cover: '', battlemaps: [], illustrations: [] };
  try {
    p.fake.setContext('campaign', 'another');
    await settle();
    assert.equal(p.$('#elements-search-input').getAttribute('type'), 'search');
    assert.equal(p.$<HTMLInputElement>('#elements-search-input').value, 'moulin', 'what was typed stays');
    assert.equal(p.$('#elements-results-title').textContent, 'Rien pour « moulin »');
    assert.deepEqual(p.found(), []);
  } finally {
    for (const id of ['campaign.listPlaces', 'campaign.listEvents', 'media.listMedia']) ANSWERS[id] = mockFixture(id);
  }
});

test('a read that fails keeps what the panel already held of it; one that succeeds again takes the message down', async () => {
  const p = await panel({ context: CONTEXT, failing: [] });
  assert.equal(p.host.querySelectorAll('#elements-groups details')[0]?.querySelectorAll('li').length, 4);
  const failing = new Set<string>(['campaign.listCharacters', 'session.listScenes']);
  const read = p.fake.shell.capability.bind(p.fake.shell);
  p.fake.shell.capability = async (id, sha, variables) => {
    if (failing.has(id)) throw new Error('down');
    return read(id, sha, variables);
  };
  p.fake.setContext('session', 's5');
  await settle();
  assert.equal(p.$('#elements-status').hidden, false);
  assert.equal(p.host.querySelectorAll('#elements-groups details')[0]?.querySelectorAll('li').length, 4, 'the characters stay');
  assert.equal(p.host.querySelectorAll('#elements-scenes details').length, 3, 'the scenes stay');
  failing.clear();
  p.fake.setContext('session', 's6');
  await settle();
  assert.equal(p.$('#elements-status').hidden, true);
});

test('a read that ends while a command is under way does not take its message away', async () => {
  const p = await panel({ context: CONTEXT });
  await p.click('#sc3-battlemap-bm-gue [data-send-map]');
  assert.equal(p.$('#elements-status').textContent, 'Battlemap envoyée aux joueurs.');
  p.fake.setContext('session', 's5');
  await settle();
  assert.equal(p.$('#elements-status').textContent, 'Battlemap envoyée aux joueurs.');
});

test('what the DM unfolded and the scene the session is at stay as they were when the panel is read again', async () => {
  const p = await panel({ context: CONTEXT });
  p.$<HTMLDetailsElement>('#elements-group-pnj').open = true;
  p.doc.dispatchEvent(new p.dom.window.CustomEvent('fdm:scene', { detail: { index: 1 } }));
  p.$<HTMLDetailsElement>('#elements-scene-sc3').open = true;
  p.fake.setContext('session', 's5');
  await settle();
  assert.deepEqual([...p.host.querySelectorAll<HTMLDetailsElement>('#elements-scenes details')].map((d) => d.open), [true, true, true]);
  assert.deepEqual([...p.host.querySelectorAll('#elements-scenes details.is-current')].map((d) => d.id), ['elements-scene-sc2']);
  assert.ok(p.$<HTMLDetailsElement>('#elements-group-pnj').open);
  assert.ok(!p.$<HTMLDetailsElement>('#elements-group-pj').open);
});

// AC1: an element listed only in scene 3 (Vesna) is visible when the panel opens and a click on it raises fdm:open
// AC2: the first scene is open and is-current when the panel opens
test('an element of a later scene is clickable at open, with no scene unfolded first', async () => {
  const p = await panel({ context: CONTEXT });
  const sc3 = p.$<HTMLDetailsElement>('#elements-scene-sc3');
  assert.ok(sc3.open && !sc3.classList.contains('is-current'));
  assert.ok(p.$<HTMLDetailsElement>('#elements-scene-sc1').open && p.$('#elements-scene-sc1').classList.contains('is-current'));
  await p.click('#sc3-character-vesna .mf-ep__open');
  assert.deepEqual(p.heard, [{ name: 'fdm:open', detail: { type: 'character', id: 'vesna' } }]);
});

test('a scene event with no scene in it changes nothing', async () => {
  const p = await panel({ context: CONTEXT });
  p.doc.dispatchEvent(new p.dom.window.CustomEvent('fdm:scene'));
  p.doc.dispatchEvent(new p.dom.window.CustomEvent('fdm:scene', { detail: { index: 'x' } }));
  assert.ok(p.$<HTMLDetailsElement>('#elements-scene-sc1').open);
});

test('a scene’s media keep the order the Capability gives, battlemaps and illustrations mixed', async () => {
  const scenes = (mockFixture('session.listScenes') as Array<Record<string, unknown>>).map((scene) => ({ ...scene, media: ['ill-moulin', 'bm-gue', 'ill-gaspard'] }));
  ANSWERS['session.listScenes'] = scenes;
  try {
    const p = await panel({ context: CONTEXT });
    assert.deepEqual([...p.host.querySelectorAll('#elements-scene-sc1 [id^="sc1-"][id*="-ill-"], #elements-scene-sc1 [id^="sc1-battlemap"]')].map((e) => e.id), ['sc1-illustration-ill-moulin', 'sc1-battlemap-bm-gue', 'sc1-illustration-ill-gaspard']);
  } finally {
    ANSWERS['session.listScenes'] = mockFixture('session.listScenes');
  }
});

test('a PC a scene lists is not among the scene’s NPCs', async () => {
  const scenes = (mockFixture('session.listScenes') as Array<Record<string, unknown>>).map((scene) => ({ ...scene, characters: ['elowen', 'gaspard'] }));
  ANSWERS['session.listScenes'] = scenes;
  try {
    const p = await panel({ context: CONTEXT });
    assert.equal(p.host.querySelectorAll('#elements-scene-sc1 [id^="sc1-character-"]').length, 1);
    assert.equal(p.host.querySelectorAll('#sc1-character-elowen').length, 0);
  } finally {
    ANSWERS['session.listScenes'] = mockFixture('session.listScenes');
  }
});

test('a session with no scene says so; a read that fails leaves the rest of the panel and says it', async () => {
  const p = await panel({ context: CONTEXT, failing: ['session.listScenes'] });
  assert.match(p.$('#elements-scenes').textContent ?? '', /Aucune scène/);
  assert.equal(p.$('#elements-status').hidden, false);
  assert.match(p.$('#elements-status').textContent ?? '', /n’a pas pu être lue/);
  assert.equal(p.host.querySelectorAll('#elements-groups details').length, 5);
});

test('typing elowen, rodeuse, LA RÔDEUSE: Elowen Ventargent stays listed, the regions give way to the results', async () => {
  const p = await panel({ context: CONTEXT });
  for (const query of ['elowen', 'rodeuse', 'LA RÔDEUSE']) {
    await p.type(query);
    assert.ok(p.found().some((n) => n.startsWith('Elowen Ventargent')), query);
    assert.equal(p.$('#elements-results').hidden, false);
    assert.equal(p.$('#elements-session').hidden, true);
    assert.equal(p.$('#elements-campaign').hidden, true);
  }
  assert.match(p.$('#elements-results-title').textContent ?? '', /^\d+ résultats? pour « LA RÔDEUSE »$/);
});

test('a blank or whitespace-only search shows the full panel again and matches nothing', async () => {
  const p = await panel({ context: CONTEXT });
  await p.type('elowen');
  for (const blank of ['', '   ', '  \t ']) {
    await p.type(blank);
    assert.equal(p.$('#elements-results').hidden, true, JSON.stringify(blank));
    assert.equal(p.$('#elements-session').hidden, false);
    assert.equal(p.$('#elements-campaign').hidden, false);
  }
});

test('a search that finds nothing says so', async () => {
  const p = await panel({ context: CONTEXT });
  await p.type('zzzzzz');
  assert.equal(p.$('#elements-results-title').textContent, 'Rien pour « zzzzzz »');
  assert.deepEqual(p.found(), []);
});

test('clicking an NPC, a place or an event tells the centre to open it, by type and id', async () => {
  const p = await panel({ context: CONTEXT });
  await p.click('#sc1-character-gaspard .mf-ep__open');
  await p.click('#sc1-place-taverne-sanglier .mf-ep__open');
  await p.click('#sc1-event-marche-noir .mf-ep__open');
  assert.deepEqual(p.heard, [
    { name: 'fdm:open', detail: { type: 'character', id: 'gaspard' } },
    { name: 'fdm:open', detail: { type: 'place', id: 'taverne-sanglier' } },
    { name: 'fdm:open', detail: { type: 'event', id: 'marche-noir' } },
  ]);
});

test('a battlemap: Ouvrir opens its instance; Envoyer submits live.showOnPlayerView with it as base', async () => {
  const p = await panel({ context: CONTEXT });
  await p.click('#sc3-battlemap-bm-gue [data-open-map].chip');
  assert.deepEqual(p.heard, [{ name: 'fdm:open-map', detail: { id: 'bmi-gue-s4' } }]);
  await p.click('#sc3-battlemap-bm-gue [data-send-map]');
  assert.equal(p.fake.sent.length, 1);
  assert.deepEqual(p.fake.sent[0], {
    dataCapability: 'live.showOnPlayerView@1',
    target: { aggregate: 'Diffusion', id: 's4' },
    payload: { kind: 'battlemap', instance: 'bmi-gue-s4', campaign: 'cendres-valombre' },
    caller: { by: 'dm', from: 'ElementsPanel', roles: [] },
  });
  assert.equal(p.$('#elements-status').textContent, 'Battlemap envoyée aux joueurs.');
});

test('clicking the name of a battlemap opens its instance too', async () => {
  const p = await panel({ context: CONTEXT });
  await p.click('#sc3-battlemap-bm-gue .mf-ep__open');
  assert.deepEqual(p.heard, [{ name: 'fdm:open-map', detail: { id: 'bmi-gue-s4' } }]);
});

test('a battlemap the session holds no instance of can be neither opened nor sent', async () => {
  const p = await panel({ context: CONTEXT });
  const row = p.$('#all-battlemap-bm-moulin');
  assert.ok(row.querySelector<HTMLButtonElement>('.mf-ep__open')?.disabled);
  assert.equal(row.querySelectorAll('.chip').length, 0);
  await p.click('#all-battlemap-bm-moulin .mf-ep__open');
  assert.deepEqual(p.heard, []);
  assert.deepEqual(p.fake.sent, []);
});

test('an illustration: Scène submits live.showOnPlayerView, Fenêtre live.showWindowOnPlayerView', async () => {
  const p = await panel({ context: CONTEXT });
  await p.click('#sc1-illustration-ill-gaspard [data-send-scene]');
  await p.click('#sc1-illustration-ill-gaspard [data-send-window]');
  assert.deepEqual(p.fake.sent.map((c) => [c.dataCapability, c.payload]), [
    ['live.showOnPlayerView@1', { kind: 'illustration', media: 'ill-gaspard', campaign: 'cendres-valombre' }],
    ['live.showWindowOnPlayerView@1', { media: 'ill-gaspard', campaign: 'cendres-valombre' }],
  ]);
  assert.match(p.$('#elements-status').textContent ?? '', /^« Gaspard, le tavernier » envoyée aux joueurs en fenêtre\.$/);
});

test('nothing is sent to the players but by a click on an Envoyer', async () => {
  const p = await panel({ context: CONTEXT });
  await p.type('gaspard');
  await p.click('#found-character-gaspard .mf-ep__open');
  assert.deepEqual(p.fake.sent, []);
});

test('a command is followed until the server has applied it', async () => {
  const p = await panel({ context: CONTEXT, status: ['pending', 'pending', 'applied'] });
  await p.click('#sc1-illustration-ill-gaspard [data-send-scene]');
  assert.equal(p.$('#elements-status').textContent, '« Gaspard, le tavernier » envoyée aux joueurs en scène.');
});

test('a command the server refuses says why; one that never settles, or never leaves, says so', async () => {
  const refused = await panel({ context: CONTEXT, status: 'rejected', violations: ['base-coherent'] });
  await refused.click('#sc3-battlemap-bm-gue [data-send-map]');
  assert.match(refused.$('#elements-status').textContent ?? '', /^Pas envoyé : cette carte ou cette illustration n’est pas celle de la séance\.$/);
  assert.ok(refused.$('#elements-status').classList.contains('is-error'));

  const stuck = await panel({ context: CONTEXT, status: 'pending' });
  await stuck.click('#sc3-battlemap-bm-gue [data-send-map]');
  assert.match(stuck.$('#elements-status').textContent ?? '', /n’a pas abouti \(état : pending\)/);

  const down = await panel({ context: CONTEXT, throwOnCommand: true });
  await down.click('#sc3-battlemap-bm-gue [data-send-map]');
  assert.match(down.$('#elements-status').textContent ?? '', /n’a pas pu être fait/);
});

test('without a session in the context the command still names the diffusion, with no id', async () => {
  const p = await panel({ context: { campaign: 'cendres-valombre' } });
  await p.click('#all-illustration-ill-cover [data-send-window]');
  assert.deepEqual(p.fake.sent[0]?.target, { aggregate: 'Diffusion' });
  assert.deepEqual(p.fake.sent[0]?.payload, { media: 'ill-cover', campaign: 'cendres-valombre' });
});

test('a PC is dragged by the Battlemap’s own type, which opens the map for the drop', async () => {
  const p = await panel({ context: CONTEXT });
  const data = new Map<string, string>();
  const event = new p.dom.window.Event('dragstart', { bubbles: true });
  Object.defineProperty(event, 'dataTransfer', { value: { setData: (type: string, value: string) => data.set(type, value), effectAllowed: '' } });
  p.$('#all-character-elowen').dispatchEvent(event);
  assert.equal(data.get('text/fdm-character'), 'elowen');
  assert.equal(data.get('text/plain'), 'Elowen Ventargent');
  assert.deepEqual(p.heard, [{ name: 'fdm:open-map', detail: {} }]);
  const draggable = [...p.host.querySelectorAll('[draggable="true"]')];
  assert.ok(draggable.length >= 9);
  assert.ok(draggable.every((row) => /-character-/.test(row.id)), 'only the characters can be dragged');
});

test('the scene the session reaches is marked, and opened again if the DM had folded it; the others stay as they are', async () => {
  const p = await panel({ context: CONTEXT });
  p.$<HTMLDetailsElement>('#elements-scene-sc2').open = false;
  p.$<HTMLDetailsElement>('#elements-scene-sc3').open = false;
  p.doc.dispatchEvent(new p.dom.window.CustomEvent('fdm:scene', { detail: { index: 2 } }));
  const open = [...p.host.querySelectorAll<HTMLDetailsElement>('#elements-scenes details')].map((d) => d.open);
  assert.deepEqual(open, [true, false, true]);
  assert.deepEqual([...p.host.querySelectorAll('#elements-scenes details.is-current')].map((d) => d.id), ['elements-scene-sc3']);
});

// AC2: after fdm:scene {index: 1} scene 2 is open and is-current, scene 1 stays open and clickable
test('moving to the next scene keeps the earlier one unfolded, and its elements stay clickable', async () => {
  const p = await panel({ context: CONTEXT });
  assert.ok(p.$<HTMLDetailsElement>('#elements-scene-sc1').open);
  p.doc.dispatchEvent(new p.dom.window.CustomEvent('fdm:scene', { detail: { index: 1 } }));
  assert.ok(p.$<HTMLDetailsElement>('#elements-scene-sc2').open);
  assert.ok(p.$('#elements-scene-sc2').classList.contains('is-current'));
  assert.ok(p.$<HTMLDetailsElement>('#elements-scene-sc1').open, 'scene 1 is still open');
  assert.ok(!p.$('#elements-scene-sc1').classList.contains('is-current'));
  await p.click('#sc1-character-gaspard .mf-ep__open');
  assert.deepEqual(p.heard, [{ name: 'fdm:open', detail: { type: 'character', id: 'gaspard' } }]);
});

test('every need is shown by a part that names it, every action triggered by an actuator that names it', async () => {
  const p = await panel({ context: CONTEXT });
  await p.type('gaspard');
  await p.type('');
  for (const need of manifest.needs) assert.ok([...p.host.querySelectorAll('[data-reads]')].some((e) => e.getAttribute('data-reads')?.split(' ').includes(need)), need);
  for (const action of manifest.actions) assert.ok(p.host.querySelector(`button[data-writes="${action}"]`), action);
  assert.deepEqual(manifest.props, {});
});

test('the markup is escaped: a name is text, never markup', async () => {
  ANSWERS['campaign.listPlaces'] = [{ id: 'x"y', name: '<img src=x onerror=alert(1)>', aliases: [] }];
  try {
    const p = await panel({ context: CONTEXT });
    assert.equal(p.host.querySelectorAll('img').length, 0);
    assert.ok(p.host.textContent?.includes('<img src=x onerror=alert(1)>'));
  } finally {
    ANSWERS['campaign.listPlaces'] = mockFixture('campaign.listPlaces');
  }
});

test('the panel’s look gives nothing a width: long names and chips wrap, so nothing overflows in a wider or narrower column', () => {
  assert.doesNotMatch(STYLE, /(?:width|height)\s*:\s*[\d.]+(?:px|rem|em|ch|vw|pt)/, 'no fixed width');
  assert.doesNotMatch(STYLE, /overflow(?:-x)?\s*:\s*(?:hidden|clip)/, 'nothing is clipped');
  assert.match(STYLE, /\.mf-ep\s*\{[^}]*min-width: 0/);
  assert.match(STYLE, /\.mf-ep__item\s*\{[^}]*flex-wrap: wrap/);
  assert.match(STYLE, /\.mf-ep__open\s*\{[^}]*overflow-wrap: anywhere/);
  assert.match(STYLE, /summary\s*\{[^}]*overflow-wrap: anywhere/);
  assert.doesNotMatch(STYLE, /#[0-9a-fA-F]{3,8}\b|rgba?\(|\d+px/, 'every value is a token');
});
