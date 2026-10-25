import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { downloadIn, mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const mock = (path: string): string => readFileSync(new URL(`../../../../mock/${path}`, import.meta.url), 'utf8');

/** The mock-up's fixture of the Capability: the campaigns the home page opens on. */
const fixture = JSON.parse(mock('fixtures/campaign.listCampaigns.json')) as Array<Record<string, unknown>>;

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  idempotencyKey?: string;
  targetId?: unknown;
  target?: unknown;
}

type Outcome = { status: string; violations?: string[]; attachment?: unknown } | { refused: string } | { offline: true };

interface Options {
  /** What applying each command does to the campaigns, and the state it ends in. */
  apply?: (sent: Sent, campaigns: Array<Record<string, unknown>>) => Outcome;
  /** How many lookups a command stays queued for. */
  later?: number;
}

const ARCHIVE = { file: 'les-cendres-de-valombre-2026-10-14T10-00-00Z.json', content: { format: 'campaign-archive', version: 1, campaign: { id: 'cendres-valombre' } } };

/** The server as the DataGuard answers it: an export answers its archive, a create and an import add a campaign. */
const defaults = (sent: Sent, campaigns: Array<Record<string, unknown>>): Outcome => {
  if (sent.dataCapability === 'campaign.exportCampaign@1') return { status: 'applied', attachment: ARCHIVE };
  if (sent.dataCapability === 'campaign.createCampaign@1') {
    campaigns.unshift({ id: 'new-1', name: sent.payload.name, type: sent.payload.type, art: null, glyph: sent.payload.glyph, pitch: sent.payload.pitch, sessions: sent.payload.type === 'oneshot' ? 1 : 0, lastPlayed: null, next: null });
  } else if (sent.dataCapability === 'campaign.importCampaign@1') {
    campaigns.unshift({ id: 'copy-1', name: 'Les Cendres de Valombre (copie)', type: 'campagne', art: 'ember', glyph: 'V', pitch: 'Importée', sessions: 4, lastPlayed: null, next: null });
  }
  return { status: 'applied' };
};

function fakeShell(options: Options = {}) {
  const campaigns = structuredClone(fixture);
  const sent: Sent[] = [];
  const reads: Array<Record<string, unknown> | undefined> = [];
  let asked = 0;
  let outcome: Outcome | undefined;
  const settledOf = (o: Outcome | undefined, later: boolean) => {
    const done = o !== undefined && 'status' in o ? o : { status: 'queued' };
    return later ? { status: 'queued', violations: [] } : { status: done.status, violations: 'violations' in done ? (done.violations ?? []) : [], attachment: 'attachment' in done ? done.attachment : undefined };
  };
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(id, 'campaign.listCampaigns');
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push(variables);
      return { asOf: 1, data: structuredClone(campaigns) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      outcome = (options.apply ?? defaults)(command, campaigns);
      if ('offline' in outcome) throw new Error('offline');
      if ('refused' in outcome) throw Object.assign(new Error('the server answered 422'), { status: 422, body: { error: outcome.refused } });
      return { commandId: 'c1', result: { commandId: 'c1', ...settledOf(outcome, options.later !== undefined) } };
    },
    async commandResult(commandId) {
      asked += 1;
      return { result: { commandId, ...settledOf(outcome, asked <= (options.later ?? 0)) } };
    },
  };
  return { shell, sent, reads, campaigns };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 5));

async function list(options?: Options, host?: { dom: JSDOM; element: HTMLElement }) {
  const dom = host?.dom ?? new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const { document } = dom.window;
  const element = host?.element ?? (document.querySelector('[data-component]') as HTMLElement);
  const fake = fakeShell(options);
  const saved: Array<{ file: string; content: unknown }> = [];
  mount(element, {}, fake.shell, timing, (file, content) => saved.push({ file, content }));
  await settle();
  const text = (selector: string): string => (document.querySelector(selector)?.textContent ?? '').replace(/\s+/g, ' ').trim();
  const click = async (selector: string) => {
    const target = document.querySelector(selector);
    assert.ok(target, `${selector} is in the list`);
    target.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const type = (selector: string, value: string) => {
    (document.querySelector(selector) as HTMLInputElement).value = value;
  };
  /** The DM chooses a file in the FileDrop. */
  const choose = async (name: string, content: string) => {
    const input = document.querySelector<HTMLInputElement>('#campaign-import-file') as HTMLInputElement;
    Object.defineProperty(input, 'files', { value: [new dom.window.File([content], name, { type: 'application/json' })], configurable: true });
    input.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
    await new Promise<void>((resolve) => setTimeout(resolve, 20));
  };
  const names = (): string[] => [...document.querySelectorAll('.kit-card h3')].map((h) => h.textContent ?? '');
  return { dom, document, element, text, click, type, choose, names, saved, ...fake };
}

test('on the fixture, one card per campaign draws what the mock-up draws', async () => {
  const { document, text, names } = await list();
  const drawing = new JSDOM(mock('components/CampaignList.html')).window.document;
  // Every part of the mock-up's fragment is in the list, under the same id.
  for (const part of drawing.querySelectorAll('[id]')) assert.ok(document.getElementById(part.id), `#${part.id} is in the list`);
  assert.deepEqual(names(), ['Les Cendres de Valombre', 'La Nuit du Carnaval Pourpre', 'Les Héritiers de la Tour d’Ébène']);
  assert.equal(document.querySelectorAll('#campaign-cards > div').length, 3);
  const valombre = '#campaign-cendres-valombre';
  assert.equal(text(`${valombre} .art__glyph`), 'V');
  assert.ok(document.querySelector(`${valombre} .art--ember`));
  assert.equal(text(`${valombre} .kit-card p`), 'Un bourg couvert de cendres, un moulin incendié, et la Tour d’Ébène qui tire les ficelles.');
  assert.equal(text(`${valombre} .kit-badge span`), 'Campagne');
  assert.match(text(`${valombre} .mf-campaigns__meta`), /4 séances.*jouée le 3 octobre 2026/);
  assert.match(text(`${valombre} .small`), /Prochaine séance : 17 octobre 2026/);
  assert.equal(text('#campaign-nuit-carnaval .kit-badge span'), 'Oneshot');
  assert.match(text('#campaign-nuit-carnaval .mf-campaigns__meta'), /1 séance.*jouée le 11 juillet 2026/);
  assert.equal(document.querySelector('#campaign-nuit-carnaval .small'), null, 'no next session, none shown');
  assert.deepEqual([...document.querySelectorAll('.kit-card .actions button')].slice(0, 2).map((b) => b.textContent), ['Ouvrir', 'Exporter']);
});

test('the parts name what they show and what they trigger, by identifier', async () => {
  const { document } = await list();
  assert.equal(document.getElementById('campaign-cards')?.dataset.reads, 'campaign.listCampaigns');
  assert.equal(document.querySelector<HTMLElement>('#campaign-cendres-valombre')?.dataset.reads, 'campaign.listCampaigns');
  assert.equal(document.getElementById('campaign-new')?.dataset.writes, 'campaign.createCampaign');
  assert.equal(document.getElementById('campaign-import')?.dataset.writes, 'campaign.importCampaign');
  const exporters = [...document.querySelectorAll<HTMLElement>('[data-action="export"]')];
  assert.equal(exporters.length, 3);
  for (const button of exporters) assert.equal(button.dataset.writes, 'campaign.exportCampaign');
});

test('no campaign yet: the list says so, and the two ways to get one are there', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const shell: Shell = { capability: async () => ({ asOf: 1, data: [] }), command: async () => ({}), commandResult: async () => ({}) };
  mount(element, {}, shell, timing);
  await settle();
  assert.match(element.querySelector('#campaign-empty')?.textContent ?? '', /Aucune campagne/);
  assert.ok(element.querySelector('#campaign-new') && element.querySelector('#campaign-import'));
});

test('a list that cannot be read says so', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const shell: Shell = { capability: async () => Promise.reject(new Error('down')), command: async () => ({}), commandResult: async () => ({}) };
  mount(element, {}, shell, timing);
  await settle();
  assert.equal(element.querySelector('#campaign-status')?.textContent, 'Impossible de lire les campagnes.');
});

test('Exporter sends the export of that campaign alone, and hands over the archive the command answered once applied', async () => {
  const { click, sent, saved, text } = await list();
  await click('#campaign-nuit-carnaval [data-action="export"]');
  assert.equal(sent.length, 1);
  assert.equal(sent[0]?.dataCapability, 'campaign.exportCampaign@1');
  assert.deepEqual(sent[0]?.payload, { id: 'nuit-carnaval' });
  assert.deepEqual(saved, [ARCHIVE], 'the file and the content the command answered, as they are');
  assert.match(text('#campaign-status'), /Archive « les-cendres-de-valombre-2026-10-14T10-00-00Z\.json » enregistrée\./);
});

test('an export is offered for download only once applied: queued, nothing yet', async () => {
  const waits = await list({ later: 2 });
  await waits.click('#campaign-cendres-valombre [data-action="export"]');
  assert.deepEqual(waits.saved, [ARCHIVE], 'followed until applied');
  const never = await list({ later: 99 });
  await never.click('#campaign-cendres-valombre [data-action="export"]');
  assert.deepEqual(never.saved, [], 'a command that is still queued has no archive to hand over');
  assert.match(never.text('#campaign-status'), /Export non appliqué : état queued\./);
});

test('a rejected export, an export with no archive and a server that is down hand over nothing and say why', async () => {
  const rejected = await list({ apply: () => ({ status: 'rejected', violations: ['campaign-not-found'] }) });
  await rejected.click('#campaign-cendres-valombre [data-action="export"]');
  assert.deepEqual(rejected.saved, []);
  assert.equal(rejected.text('#campaign-status'), 'Export refusé : campaign-not-found.');
  const bare = await list({ apply: () => ({ status: 'applied' }) });
  await bare.click('#campaign-cendres-valombre [data-action="export"]');
  assert.deepEqual(bare.saved, []);
  assert.equal(bare.text('#campaign-status'), 'Export effectué, mais l’archive n’a pas été reçue.');
  const down = await list({ apply: () => ({ offline: true }) });
  await down.click('#campaign-cendres-valombre [data-action="export"]');
  assert.equal(down.text('#campaign-status'), 'L’export a échoué.');
});

test('an archive that cannot be saved is said, not thrown', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const fake = fakeShell();
  mount(element, {}, fake.shell, timing, () => {
    throw new Error('disk full');
  });
  await settle();
  element.querySelector('[data-action="export"]')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(element.querySelector('#campaign-status')?.textContent, 'Export effectué, mais l’archive n’a pas pu être enregistrée.');
});

test('no export is made unless the DM clicks Exporter: mounting and reading send no command', async () => {
  const { sent, reads } = await list();
  assert.deepEqual(sent, []);
  assert.equal(reads.length, 1);
});

test('Nouvelle campagne opens the dialog; a name is required; the command carries the form, then the list is read again with the new card', async () => {
  const { document, click, type, sent, reads, names, text } = await list();
  assert.equal(document.getElementById('campaign-create-dialog'), null);
  await click('#campaign-new');
  assert.ok(document.getElementById('campaign-create-dialog'));
  assert.equal(document.getElementById('campaign-create-submit')?.dataset.writes, 'campaign.createCampaign');
  await click('#campaign-create-submit');
  assert.equal(text('#campaign-create-error'), 'Le nom est obligatoire.');
  assert.equal(sent.length, 0, 'a blank name is not sent');

  type('#campaign-create-name', ' Les Brumes de Corbière ');
  (document.querySelector('input[name="campaign-type"][value="oneshot"]') as HTMLInputElement).checked = true;
  type('#campaign-create-pitch', 'Une nuit de brume.');
  await click('#campaign-create-submit');
  assert.equal(sent.length, 1);
  assert.equal(sent[0]?.dataCapability, 'campaign.createCampaign@1');
  assert.deepEqual(sent[0]?.payload, { name: 'Les Brumes de Corbière', type: 'oneshot', pitch: 'Une nuit de brume.', glyph: 'L' });
  assert.equal(document.getElementById('campaign-create-dialog'), null, 'closed once applied');
  assert.equal(reads.length, 2, 'the list is read again');
  assert.equal(names()[0], 'Les Brumes de Corbière');
  assert.equal(names().length, 4);
  assert.match(text('#campaign-status'), /Campagne « Les Brumes de Corbière » créée\./);
});

test('a refused creation keeps the dialog open with the reason and the list as it was', async () => {
  const { document, click, type, reads, names, text } = await list({ apply: () => ({ status: 'rejected', violations: ['campaign-name-required'] }) });
  await click('#campaign-new');
  type('#campaign-create-name', 'X');
  await click('#campaign-create-submit');
  assert.ok(document.getElementById('campaign-create-dialog'));
  assert.equal(text('#campaign-create-error'), 'Création refusée : campaign-name-required.');
  assert.equal(reads.length, 1);
  assert.equal(names().length, 3);
});

test('Annuler, × and Échap close the dialogs without sending anything', async () => {
  const { document, dom, click, sent } = await list();
  await click('#campaign-new');
  await click('#campaign-create-dialog .kit-dialog__footer [data-close]');
  assert.equal(document.getElementById('campaign-create-dialog'), null);
  await click('#campaign-import');
  await click('#campaign-import-dialog .kit-dialog__close');
  assert.equal(document.getElementById('campaign-import-dialog'), null);
  await click('#campaign-import');
  document.querySelector('#campaign-import-dialog')?.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.equal(document.getElementById('campaign-import-dialog'), null);
  assert.deepEqual(sent, []);
});

test('Importer une archive opens a dialog with a FileDrop; importing is refused until a file is an archive', async () => {
  const { document, click, choose, sent, text } = await list();
  await click('#campaign-import');
  const dialog = document.getElementById('campaign-import-dialog');
  assert.ok(dialog?.querySelector('.kit-dialog__window[role="dialog"]'));
  assert.ok(dialog?.querySelector('.kit-filedrop #campaign-import-file'));
  assert.equal(document.getElementById('campaign-import-submit')?.dataset.writes, 'campaign.importCampaign');
  const submit = document.getElementById('campaign-import-submit') as HTMLButtonElement;
  assert.equal(submit.disabled, true, 'nothing to import yet');
  await click('#campaign-import-submit');
  assert.equal(sent.length, 0);

  await choose('photo.json', 'ceci n’est pas du json');
  assert.ok(document.getElementById('campaign-import-field')?.classList.contains('has-error'));
  assert.equal(submit.disabled, true);

  await choose('archive.json', JSON.stringify(ARCHIVE.content));
  const field = document.getElementById('campaign-import-field');
  assert.ok(field?.classList.contains('has-file') && !field.classList.contains('has-error'));
  assert.equal(text('.kit-filedrop__name'), 'archive.json');
  assert.equal(submit.disabled, false);
});

test('dropping an archive and confirming sends campaign.importCampaign with it; once applied the list is read again and shows the copy beside the original', async () => {
  const { document, dom, click, choose, sent, reads, names, text } = await list();
  await click('#campaign-import');
  await choose('archive.json', JSON.stringify(ARCHIVE.content));
  await click('#campaign-import-submit');
  assert.equal(sent.length, 1);
  assert.equal(sent[0]?.dataCapability, 'campaign.importCampaign@1');
  assert.deepEqual(sent[0]?.payload, { archive: ARCHIVE.content }, 'the archive, and nothing that names a campaign to overwrite');
  assert.equal(sent[0]?.targetId, undefined);
  assert.equal(sent[0]?.target, undefined);
  assert.equal(document.getElementById('campaign-import-dialog'), null);
  assert.equal(reads.length, 2);
  assert.equal(names().length, 4);
  assert.ok(names().includes('Les Cendres de Valombre') && names().includes('Les Cendres de Valombre (copie)'), 'the original is still there');
  assert.match(text('#campaign-status'), /Archive importée/);
  assert.equal(dom.window.document.querySelectorAll('#campaign-cendres-valombre').length, 1);

  // A drop on the zone is a file chosen.
  await click('#campaign-import');
  const zone = document.getElementById('campaign-import-drop') as HTMLElement;
  const file = new dom.window.File([JSON.stringify(ARCHIVE.content)], 'dropped.json');
  const drop = new dom.window.Event('drop', { bubbles: true, cancelable: true });
  Object.defineProperty(drop, 'dataTransfer', { value: { files: [file] } });
  zone.dispatchEvent(drop);
  await new Promise<void>((resolve) => setTimeout(resolve, 20));
  assert.equal(text('.kit-filedrop__name'), 'dropped.json');
});

test('a rejected import shows its reason in the dialog and leaves the list as it was', async () => {
  const { document, click, choose, reads, names, text } = await list({ apply: () => ({ status: 'rejected', violations: ['archive-format-unknown'] }) });
  await click('#campaign-import');
  await choose('archive.json', JSON.stringify({ format: 'other' }));
  await click('#campaign-import-submit');
  assert.ok(document.getElementById('campaign-import-dialog'), 'the dialog stays open');
  assert.equal(text('#campaign-import-error'), 'Import refusé : archive-format-unknown.');
  assert.equal(reads.length, 1, 'the list is not read again');
  assert.equal(names().length, 3);
});

test('an import the server refuses before queueing it shows the server’s words; a server that is down says it failed', async () => {
  const refused = await list({ apply: () => ({ refused: 'format inconnu : other' }) });
  await refused.click('#campaign-import');
  await refused.choose('archive.json', JSON.stringify({ format: 'other' }));
  await refused.click('#campaign-import-submit');
  assert.equal(refused.text('#campaign-import-error'), 'Import refusé : format inconnu : other.');
  assert.equal(refused.names().length, 3);
  const down = await list({ apply: () => ({ offline: true }) });
  await down.click('#campaign-import');
  await down.choose('archive.json', JSON.stringify(ARCHIVE.content));
  await down.click('#campaign-import-submit');
  assert.equal(down.text('#campaign-import-error'), 'L’import a échoué.');
  assert.ok((down.document.getElementById('campaign-import-submit') as HTMLButtonElement).disabled === false, 'the DM may try again');
});

test('an import applied whose list cannot be read again says so', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const fake = fakeShell();
  let reads = 0;
  const shell: Shell = {
    ...fake.shell,
    capability: async (id, sha, variables) => {
      reads += 1;
      if (reads > 1) throw new Error('down');
      return fake.shell.capability(id, sha, variables);
    },
  };
  mount(element, {}, shell, timing);
  await settle();
  element.querySelector('#campaign-import')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  const input = element.querySelector<HTMLInputElement>('#campaign-import-file') as HTMLInputElement;
  Object.defineProperty(input, 'files', { value: [new dom.window.File([JSON.stringify(ARCHIVE.content)], 'a.json')] });
  input.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  await new Promise<void>((resolve) => setTimeout(resolve, 20));
  element.querySelector('#campaign-import-submit')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(element.querySelector('#campaign-status')?.textContent, 'Archive importée : une nouvelle campagne a été ajoutée. La liste n’a pas pu être relue.');
});

test('the buttons are held while a command is followed: one click is one command', async () => {
  const { click, sent, document } = await list({ later: 2 });
  const first = click('#campaign-cendres-valombre [data-action="export"]');
  document.querySelector('#campaign-nuit-carnaval [data-action="export"]')?.dispatchEvent(new document.defaultView!.MouseEvent('click', { bubbles: true }));
  await first;
  assert.equal(sent.length, 1);
});

test('mounted again in the same host, the list answers one click with one command, from the mount that holds it', async () => {
  const first = await list();
  const second = await list(undefined, { dom: first.dom, element: first.element });
  await second.click('#campaign-cendres-valombre [data-action="export"]');
  assert.equal(first.sent.length, 0, 'the first mount no longer listens');
  assert.equal(second.sent.length, 1);
});

test('a lookup that fails while a command is followed is asked again: the command was queued, it is not sent twice', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const fake = fakeShell({ later: 1 });
  let lookups = 0;
  const shell: Shell = {
    ...fake.shell,
    commandResult: async (id) => {
      lookups += 1;
      if (lookups === 1) throw new Error('blip');
      return fake.shell.commandResult(id);
    },
  };
  mount(element, {}, shell, timing);
  await settle();
  element.querySelector('#campaign-new')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  (element.querySelector('#campaign-create-name') as HTMLInputElement).value = 'Les Brumes';
  element.querySelector('#campaign-create-submit')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(fake.sent.length, 1);
  assert.ok(lookups >= 2, 'asked again after the blip');
  assert.equal(element.querySelector('#campaign-create-dialog'), null, 'applied, not failed');
  assert.equal(element.querySelector('#campaign-status')?.textContent, 'Campagne « Les Brumes » créée.');
});

test('a creation not settled in time, sent again, is the same command: the server keeps one campaign', async () => {
  const { click, type, sent, document, text } = await list({ later: 7 });
  await click('#campaign-new');
  type('#campaign-create-name', 'Les Brumes');
  await click('#campaign-create-submit');
  assert.match(text('#campaign-create-error'), /Création non appliquée/);
  assert.ok(document.getElementById('campaign-create-dialog'), 'the dialog stays open');
  await click('#campaign-create-submit');
  assert.equal(sent.length, 2);
  assert.ok(sent[0]?.idempotencyKey, 'the command carries a key');
  assert.equal(sent[1]?.idempotencyKey, sent[0]?.idempotencyKey, 'the second click is the same command');
});

test('a creation not settled in time, edited and sent again, is another command: it is not answered for the first name', async () => {
  const { click, type, sent } = await list({ later: 20 });
  await click('#campaign-new');
  type('#campaign-create-name', 'Les Brumes');
  await click('#campaign-create-submit');
  type('#campaign-create-name', 'Les Brumes du Nord');
  await click('#campaign-create-submit');
  await click('#campaign-create-submit');
  assert.equal(sent.length, 3);
  assert.notEqual(sent[1]?.idempotencyKey, sent[0]?.idempotencyKey, 'another name is another command');
  assert.equal(sent[2]?.idempotencyKey, sent[1]?.idempotencyKey, 'the edited one sent again is still one');
});

test('an import a server that is down may have queued, sent again with another archive, is another command', async () => {
  const { click, choose, sent } = await list({ apply: () => ({ offline: true }) });
  await click('#campaign-import');
  await choose('archive.json', JSON.stringify(ARCHIVE.content));
  await click('#campaign-import-submit');
  await choose('other.json', JSON.stringify({ ...ARCHIVE.content, campaign: { id: 'nuit-carnaval' } }));
  await click('#campaign-import-submit');
  assert.equal(sent.length, 2);
  assert.notEqual(sent[1]?.idempotencyKey, sent[0]?.idempotencyKey);
});

test('an import a server that is down may have queued, sent again, is the same command; a refused one, sent again, is a new one', async () => {
  let down = true;
  const offline = await list({ apply: (s, c) => (down ? { offline: true } : defaults(s, c)) });
  await offline.click('#campaign-import');
  await offline.choose('archive.json', JSON.stringify(ARCHIVE.content));
  await offline.click('#campaign-import-submit');
  down = false;
  await offline.click('#campaign-import-submit');
  assert.equal(offline.sent.length, 2);
  assert.ok(offline.sent[0]?.idempotencyKey);
  assert.equal(offline.sent[1]?.idempotencyKey, offline.sent[0]?.idempotencyKey, 'one import, not two copies');

  const refused = await list({ apply: () => ({ status: 'rejected', violations: ['archive-format-unknown'] }) });
  await refused.click('#campaign-import');
  await refused.choose('archive.json', JSON.stringify(ARCHIVE.content));
  await refused.click('#campaign-import-submit');
  await refused.click('#campaign-import-submit');
  assert.equal(refused.sent.length, 2);
  assert.notEqual(refused.sent[1]?.idempotencyKey, refused.sent[0]?.idempotencyKey, 'a refused command is over: trying again is a new one');
});

test('each dialog opened is a new command: two campaigns created one after the other are two', async () => {
  const { click, type, sent } = await list();
  for (const name of ['A', 'B']) {
    await click('#campaign-new');
    type('#campaign-create-name', name);
    await click('#campaign-create-submit');
  }
  assert.equal(sent.length, 2);
  assert.notEqual(sent[1]?.idempotencyKey, sent[0]?.idempotencyKey);
});

test('a dialog is not left while its command is followed: the reason it ends with has somewhere to land', async () => {
  const { document, dom, click, choose, text } = await list({ later: 2, apply: () => ({ status: 'rejected', violations: ['archive-format-unknown'] }) });
  await click('#campaign-import');
  await choose('archive.json', JSON.stringify({ format: 'other' }));
  const sending = click('#campaign-import-submit');
  document.querySelector('#campaign-import-dialog .kit-dialog__footer [data-close]')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  document.querySelector('#campaign-import-dialog')?.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  assert.ok(document.getElementById('campaign-import-dialog'), 'still open while the import is followed');
  await sending;
  assert.equal(text('#campaign-import-error'), 'Import refusé : archive-format-unknown.');
});

test('a file read for a dialog closed since is not the one of the dialog opened after', async () => {
  const { document, dom, click, text } = await list();
  await click('#campaign-import');
  const input = document.querySelector<HTMLInputElement>('#campaign-import-file') as HTMLInputElement;
  Object.defineProperty(input, 'files', { value: [new dom.window.File(['{"format":"campaign-archive"}'], 'old.json')], configurable: true });
  input.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  // Closed and opened again before the file is read.
  document.querySelector('#campaign-import-dialog .kit-dialog__close')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  document.querySelector('#campaign-import')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  assert.equal((document.getElementById('campaign-import-submit') as HTMLButtonElement).disabled, true, 'nothing chosen in this dialog');
  assert.equal(text('.kit-filedrop__name'), '');
  assert.ok(!document.getElementById('campaign-import-field')?.classList.contains('has-file'));
});

test('a read that outlives its mount does not draw over the mount that replaced it', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  let late: (data: unknown) => void = () => undefined;
  const slow: Shell = {
    capability: () => new Promise((resolve) => (late = (data) => resolve({ asOf: 1, data }))),
    command: async () => ({}),
    commandResult: async () => ({}),
  };
  mount(element, {}, slow, timing);
  mount(element, {}, fakeShell().shell, timing);
  await settle();
  late([{ id: 'old', name: 'Vieille liste' }]);
  await settle();
  assert.deepEqual([...element.querySelectorAll('.kit-card h3')].map((h) => h.textContent), ['Les Cendres de Valombre', 'La Nuit du Carnaval Pourpre', 'Les Héritiers de la Tour d’Ébène']);
});

test('a command that outlives its mount neither frees the buttons of the mount that replaced it nor hands it an archive', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignList"></div></body></html>', { url: 'http://shell.test/campagnes.html' });
  const element = dom.window.document.querySelector('[data-component]') as HTMLElement;
  const exportOn = (id: string) => element.querySelector(`#campaign-${id} [data-action="export"]`)?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  const holding = (answered: (resolve: (value: unknown) => void) => void): Shell => ({
    capability: async () => ({ asOf: 1, data: structuredClone(fixture) }),
    command: () => new Promise((resolve) => answered(resolve)),
    commandResult: async () => ({}),
  });
  let late: (value: unknown) => void = () => undefined;
  const saved: string[] = [];
  mount(element, {}, holding((resolve) => (late = resolve)), timing, (file) => saved.push(file));
  await settle();
  exportOn('cendres-valombre');
  await settle();
  // The mount that replaces it follows a command of its own: its buttons are held.
  mount(element, {}, holding(() => undefined), timing, (file) => saved.push(`second ${file}`));
  await settle();
  exportOn('nuit-carnaval');
  await settle();
  const held = () => [...element.querySelectorAll<HTMLButtonElement>('[data-writes]')].map((b) => b.disabled);
  assert.ok(held().length > 0 && held().every(Boolean), 'the second mount holds its buttons');
  late({ commandId: 'c1', result: { commandId: 'c1', status: 'applied', violations: [], attachment: ARCHIVE } });
  await settle();
  assert.ok(held().every(Boolean), 'the first mount frees nothing');
  assert.deepEqual(saved, [], 'no archive handed over by a mount replaced');
});

test('a file read for a mount replaced since does not arm the import of the mount that replaced it', async () => {
  const first = await list();
  await first.click('#campaign-import');
  const input = first.document.querySelector<HTMLInputElement>('#campaign-import-file') as HTMLInputElement;
  Object.defineProperty(input, 'files', { value: [new first.dom.window.File([JSON.stringify(ARCHIVE.content)], 'old.json')], configurable: true });
  input.dispatchEvent(new first.dom.window.Event('change', { bubbles: true }));
  // Mounted again, its own import dialog open, before the file is read.
  const shell = fakeShell().shell;
  mount(first.element, {}, shell, timing);
  first.element.querySelector('#campaign-import')?.dispatchEvent(new first.dom.window.MouseEvent('click', { bubbles: true }));
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  assert.ok(first.element.querySelector('#campaign-import-dialog'), 'the second mount’s dialog is open');
  assert.equal((first.element.querySelector('#campaign-import-submit') as HTMLButtonElement).disabled, true, 'nothing chosen in this mount');
  assert.equal(first.text('.kit-filedrop__name'), '');
});

test('the download is the archive as a JSON file under the name the export gave, its address released later', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body></body></html>', { url: 'http://shell.test/campagnes.html' });
  const win = dom.window as unknown as { URL: { createObjectURL(blob: Blob): string; revokeObjectURL(url: string): void } } & Window;
  const made: Blob[] = [];
  const revoked: string[] = [];
  win.URL.createObjectURL = (blob) => (made.push(blob), 'blob:archive');
  win.URL.revokeObjectURL = (url) => void revoked.push(url);
  const clicked: Array<{ href: string; download: string }> = [];
  dom.window.HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
    clicked.push({ href: this.href, download: this.download });
  };
  downloadIn(dom.window.document)('../valombre:1.json', ARCHIVE.content);
  assert.deepEqual(clicked, [{ href: 'blob:archive', download: '..-valombre-1.json' }]);
  assert.equal(dom.window.document.querySelector('a'), null, 'the link does not stay in the page');
  assert.deepEqual(revoked, [], 'not before the browser has started the download');
  const text = await new Promise<string>((resolve) => {
    const reader = new dom.window.FileReader();
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
    reader.readAsText(made[0] as unknown as File);
  });
  assert.deepEqual(JSON.parse(text), ARCHIVE.content);
  assert.equal(made[0]?.type, 'application/json');
  assert.throws(() => downloadIn({ defaultView: null } as unknown as Document)('a.json', {}), /no window/);
  dom.window.close();
});
