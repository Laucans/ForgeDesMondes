import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { downloadIn, mount, type Save, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

const mock = (path: string): string => readFileSync(new URL(`../../../../mock/${path}`, import.meta.url), 'utf8');

/** The mock-up's fixture of the Capability: the campaign a panel is opened on. */
const fixture = JSON.parse(mock('fixtures/campaign.getCampaign.json')) as Record<string, unknown> & { counts: Record<string, number> };

interface Sent {
  dataCapability: string;
  payload: Record<string, string>;
}

interface Options {
  /** What the Capability answers; the fixture by default. */
  read?: () => unknown;
  /** What applying the export does to the campaign, and the state it ends in. */
  apply?: (data: Record<string, unknown>) => { status: string; violations?: string[] };
  /** How many lookups a command stays queued for. */
  later?: number;
  /** What an applied export answers as its attachment; the campaign's archive by default. */
  attachment?: unknown;
  /** What saving the archive does; it keeps the file by default. */
  save?: Save;
}

const ARCHIVE = { file: 'les-cendres-de-valombre-2026-10-18T10-00-00Z.json', content: { format: 'campaign-archive', campaign: { name: 'Les Cendres de Valombre' } } };

/** A shell over a small server: it answers the campaign, and applies the export as the DataGuard would. */
function fakeShell(options: Options = {}) {
  const data: Record<string, unknown> = structuredClone(fixture);
  const sent: Sent[] = [];
  const reads: Array<Record<string, unknown> | undefined> = [];
  let asked = 0;
  const apply = options.apply ?? ((campaign: Record<string, unknown>) => ((campaign.lastExport = '2026-10-18'), { status: 'applied' }));
  let outcome: { status: string; violations?: string[] } | undefined;
  const attachment = () => (outcome?.status === 'applied' ? ('attachment' in options ? options.attachment : ARCHIVE) : undefined);
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(id, 'campaign.getCampaign');
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push(variables);
      return { asOf: 1, data: options.read ? options.read() : structuredClone(data) };
    },
    async command(body) {
      sent.push(body as Sent);
      outcome = apply(data);
      return { commandId: 'c1', result: { status: options.later === undefined ? outcome.status : 'queued', violations: outcome.violations ?? [], attachment: options.later === undefined ? attachment() : undefined } };
    },
    async commandResult(commandId) {
      asked += 1;
      const done = asked > (options.later ?? 0);
      return { result: { commandId, status: done ? outcome?.status : 'queued', violations: outcome?.violations ?? [], attachment: done ? attachment() : undefined } };
    },
  };
  return { shell, sent, reads, data };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function panel(options?: Options, props: Record<string, unknown> = { campaign: 'cendres-valombre' }, search = '') {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="CampaignOverview"></div></body></html>', { url: `http://shell.test/campagne.html${search}` });
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const saved: Array<{ file: string; content: unknown }> = [];
  mount(host, props, fake.shell, timing, options?.save ?? ((file, content) => void saved.push({ file, content })));
  await settle();
  // The words of a part, one space where a tag was.
  const text = (id: string): string => (document.getElementById(id)?.innerHTML ?? '').replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
  const click = async (id: string) => {
    document.getElementById(id)?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  return { document, host, text, click, saved, ...fake };
}

test('on the fixture, the panel draws what the mock-up draws', async () => {
  const { document, text } = await panel();
  const drawing = new JSDOM(mock('components/CampaignOverview.html')).window.document;
  // Every part of the mock-up's fragment is in the panel, under the same id.
  for (const part of drawing.querySelectorAll('[id]')) assert.ok(document.getElementById(part.id), `#${part.id} is in the panel`);
  assert.equal(text('overview-title'), 'Les Cendres de Valombre');
  assert.equal(text('overview-pitch'), fixture.pitch);
  assert.equal(text('overview-type'), 'Campagne Prochaine : samedi 17 octobre');
  assert.equal(text('overview-counts'), '4 séances 4 joueurs 9 personnages 4 lieux 3 événements 7 médias 12 sons');
  assert.equal(text('overview-launch'), 'Lancer la séance 4 en direct');
  assert.equal(text('overview-prepare'), 'Préparer la séance 4');
  assert.equal(text('overview-export'), 'Exporter l’archive');
  assert.equal(text('overview-export-status'), 'Dernier export : 4 octobre 2026');
  assert.equal(document.querySelector('.art--ember .art__glyph')?.textContent, 'V');
  assert.equal(document.querySelector('#overview-cover [role="img"]')?.getAttribute('aria-label'), 'Couverture : Les Cendres de Valombre');
});

test('it uses the mock-up\'s classes and the kit\'s, and no literal value', async () => {
  const { host, document } = await panel();
  for (const selector of ['section.mf-overview', '.mf-overview__cover', '.mf-overview__body', '.mf-overview__counts', '.kit-badge .primary', '.kit-badge .accent', '.kit-button.kit-button--bare .primary', '.kit-button .secondary', '.kit-button .ghost']) {
    assert.ok(host.querySelector(selector), selector);
  }
  const css = document.querySelector('style[data-micro-ui="CampaignOverview"]')?.textContent ?? '';
  assert.ok(css.length > 0);
  // A letter-spacing and the prelude of a @media are shapes, as the design-system gate reads them; every other colour, size and shadow is a token.
  const declarations = css.replace(/letter-spacing:[^;]*;/g, '').replace(/@media[^{]*/g, '');
  assert.equal(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(|\d(px|rem|em)\b/i.test(declarations), false, 'every colour, size and shadow is a token');
});

test('every part that shows the campaign reads campaign.getCampaign, and the export button writes campaign.exportCampaign', async () => {
  const { host } = await panel();
  for (const id of ['campaign-overview', 'overview-cover', 'overview-type', 'overview-title', 'overview-pitch', 'overview-counts', 'overview-export-status']) {
    assert.equal(host.querySelector(`#${id}`)?.getAttribute('data-reads'), 'campaign.getCampaign', id);
  }
  assert.equal(host.querySelector('#overview-export')?.getAttribute('data-writes'), 'campaign.exportCampaign');
  assert.equal(host.querySelectorAll('[data-writes]').length, 1, 'the one button that writes');
});

test('the Capability is asked for the campaign opened, the prop first and the address otherwise', async () => {
  const byProp = await panel(undefined, { campaign: 'c1' }, '?id=c2');
  assert.deepEqual(byProp.reads, [{ id: 'c1' }]);
  const byAddress = await panel(undefined, {}, '?id=c2');
  assert.deepEqual(byAddress.reads, [{ id: 'c2' }]);
});

test('no campaign opened: nothing is read, nothing can be exported', async () => {
  const { reads, text, document } = await panel(undefined, {});
  assert.deepEqual(reads, []);
  assert.equal(text('overview-export-status'), 'Aucune campagne ouverte.');
  assert.equal(document.getElementById('overview-export'), null);
});

test('an unknown campaign (the Capability answers null) is said, not drawn', async () => {
  const { text, document } = await panel({ read: () => null });
  assert.equal(text('overview-export-status'), 'Campagne introuvable.');
  assert.equal(document.getElementById('overview-export'), null);
});

test('a campaign the Capability cannot read is said', async () => {
  const { text } = await panel({
    read: () => {
      throw new Error('down');
    },
  });
  assert.equal(text('overview-export-status'), 'Impossible de lire la campagne.');
});

test('what the Capability does not answer yet is not drawn: no next session, only the counts it has', async () => {
  const answer = { id: 'c1', name: 'Un oneshot', type: 'oneshot', art: 'dusk', glyph: 'U', pitch: null, cover: null, createdAt: '2026-08-20', lastExport: null, counts: { players: 2, characters: 3, places: 1, events: 0 } };
  const { text, document } = await panel({ read: () => answer });
  assert.equal(text('overview-type'), 'Oneshot');
  assert.equal(text('overview-counts'), '2 joueurs 3 personnages 1 lieux 0 événements');
  assert.equal(document.getElementById('overview-launch'), null);
  assert.equal(document.getElementById('overview-prepare'), null);
  assert.equal(text('overview-export'), 'Exporter l’archive');
  assert.equal(text('overview-export-status'), 'Jamais exporté');
});

test('the DM exports: one command, by identifier, and the date shown is the one the campaign now holds', async () => {
  const { click, sent, reads, text } = await panel();
  assert.equal(text('overview-export-status'), 'Dernier export : 4 octobre 2026');
  await click('overview-export');
  assert.deepEqual(sent, [{ dataCapability: 'campaign.exportCampaign@1', payload: { id: 'cendres-valombre' }, caller: { by: 'dm', from: 'CampaignOverview', roles: [] } }]);
  assert.equal(reads.length, 2, 'the campaign is read again after the export');
  assert.equal(text('overview-export-status'), 'Dernier export : 18 octobre 2026');
});

test('the date is what is read back, never one the panel made up', async () => {
  const { click, text } = await panel({ apply: () => ({ status: 'applied' }) });
  await click('overview-export');
  assert.equal(text('overview-export-status'), 'Dernier export : 4 octobre 2026');
});

test('a command still queued is followed until the server has applied it', async () => {
  const { click, text, reads } = await panel({ later: 2 });
  await click('overview-export');
  await settle();
  assert.equal(text('overview-export-status'), 'Dernier export : 18 octobre 2026');
  assert.equal(reads.length, 2);
});

test('while the export runs the button holds still: a second click sends nothing', async () => {
  const { click, sent, document } = await panel({ later: 3 });
  const pending = click('overview-export');
  assert.equal((document.getElementById('overview-export') as HTMLButtonElement).disabled, true);
  await click('overview-export');
  await pending;
  await settle();
  assert.equal(sent.length, 1);
  assert.equal((document.getElementById('overview-export') as HTMLButtonElement).disabled, false);
});

test('a refused export says why and leaves the date as it was', async () => {
  const { click, text, reads } = await panel({ apply: () => ({ status: 'rejected', violations: ['export-stamps-date'] }) });
  await click('overview-export');
  assert.equal(text('overview-export-status'), 'Export refusé : export-stamps-date. Dernier export : 4 octobre 2026');
  assert.equal(reads.length, 1, 'nothing to read again');
});

test('a panel mounted again in the same host exports once per click', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>', { url: 'http://shell.test/' });
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const { shell, sent } = fakeShell();
  mount(host, { campaign: 'cendres-valombre' }, shell, timing);
  mount(host, { campaign: 'cendres-valombre' }, shell, timing);
  await settle();
  host.querySelector('#overview-export')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(sent.length, 1);
});

test('after an export the whole panel shows what was read again, not only the date', async () => {
  let reads = 0;
  const { click, text } = await panel({
    read: () => {
      reads += 1;
      return { ...structuredClone(fixture), name: reads > 1 ? 'Valombre, la suite' : fixture.name, lastExport: reads > 1 ? '2026-10-18' : '2026-10-04' };
    },
  });
  await click('overview-export');
  assert.equal(text('overview-title'), 'Valombre, la suite');
  assert.equal(text('overview-export-status'), 'Dernier export : 18 octobre 2026');
});

test('an export that never settles is said, and the old date is kept', async () => {
  const { click, text } = await panel({ later: 99 });
  await click('overview-export');
  await settle();
  assert.equal(text('overview-export-status'), 'L’export n’est pas encore appliqué (état : queued). Dernier export : 4 octobre 2026');
});

test('an export the server cannot take is said, and the button comes back', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div></div></body></html>', { url: 'http://shell.test/' });
  const host = dom.window.document.querySelector('div') as HTMLElement;
  const shell: Shell = {
    capability: async () => ({ asOf: 1, data: structuredClone(fixture) }),
    command: async () => {
      throw new Error('down');
    },
    commandResult: async () => ({}),
  };
  mount(host, { campaign: 'cendres-valombre' }, shell, timing);
  await settle();
  host.querySelector('#overview-export')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(host.querySelector('#overview-export-status')?.textContent, 'L’export a échoué. Dernier export : 4 octobre 2026');
  assert.equal((host.querySelector('#overview-export') as HTMLButtonElement).disabled, false);
});

test('applied, but the date cannot be read again: the old date is not passed off as the new one', async () => {
  let reads = 0;
  const { click, text } = await panel({
    read: () => {
      reads += 1;
      if (reads > 1) throw new Error('down');
      return structuredClone(fixture);
    },
  });
  await click('overview-export');
  assert.equal(text('overview-export-status'), 'Export effectué, mais la date n’a pas pu être relue. Dernier export : 4 octobre 2026');
});

// AC1: an applied export whose result carries an attachment is saved under its file name, whether it was applied at once or after being followed
test('the archive the applied export answers is saved, once, under the name it gave', async () => {
  const now = await panel();
  await now.click('overview-export');
  assert.deepEqual(now.saved, [ARCHIVE]);
  const later = await panel({ later: 2 });
  await later.click('overview-export');
  await settle();
  assert.deepEqual(later.saved, [ARCHIVE]);
  assert.equal(later.text('overview-export-status'), 'Dernier export : 18 octobre 2026', 'a saved file leaves the date alone in the status');
});

// AC2: a refused or pending export saves no file and the DM reads why
test('a refused, a pending or a failed export saves no file and says why', async () => {
  const refused = await panel({ apply: () => ({ status: 'rejected', violations: ['export-stamps-date'] }), attachment: ARCHIVE });
  await refused.click('overview-export');
  assert.deepEqual(refused.saved, []);
  assert.match(refused.text('overview-export-status'), /^Export refusé : export-stamps-date\./);
  const pending = await panel({ later: 99, attachment: ARCHIVE });
  await pending.click('overview-export');
  await settle();
  assert.deepEqual(pending.saved, []);
  assert.match(pending.text('overview-export-status'), /^L’export n’est pas encore appliqué \(état : queued\)/);
});

// AC1: the file is the DM's to keep even when the date cannot be read again
test('the archive is saved even when the date cannot be read again, and both are said', async () => {
  let reads = 0;
  const { click, text, saved } = await panel({
    read: () => {
      reads += 1;
      if (reads > 1) throw new Error('down');
      return structuredClone(fixture);
    },
  });
  await click('overview-export');
  assert.deepEqual(saved, [ARCHIVE]);
  assert.equal(text('overview-export-status'), 'Export effectué, mais la date n’a pas pu être relue. Dernier export : 4 octobre 2026');
});

test('an applied export that answers no usable archive saves nothing and says so, the date still read again', async () => {
  for (const attachment of [undefined, null, { file: '', content: {} }, { file: 'a.json' }, 'a.json']) {
    const { click, text, saved } = await panel({ attachment });
    await click('overview-export');
    assert.deepEqual(saved, []);
    assert.equal(text('overview-export-status'), 'Export effectué, mais le serveur n’a pas rendu l’archive. Dernier export : 18 octobre 2026');
  }
});

test('a file the browser cannot be handed is said, and the date is still the one read back', async () => {
  const { click, text } = await panel({
    save: () => {
      throw new Error('no window');
    },
  });
  await click('overview-export');
  assert.equal(text('overview-export-status'), 'Export effectué, mais le fichier n’a pas pu être enregistré. Dernier export : 18 octobre 2026');
  assert.equal(text('overview-title'), 'Les Cendres de Valombre');
});

// AC1: the JSON Blob, the safe file name, the a-download click, the object URL revoked afterwards
test('the download is the archive as a JSON file under a safe name, its address released later', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body></body></html>', { url: 'http://shell.test/campagne.html' });
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
  const body = await new Promise<string>((resolve) => {
    const reader = new dom.window.FileReader();
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
    reader.readAsText(made[0] as unknown as File);
  });
  assert.deepEqual(JSON.parse(body), ARCHIVE.content);
  assert.equal(made[0]?.type, 'application/json');
  assert.throws(() => downloadIn({ defaultView: null } as unknown as Document)('a.json', {}), /no window/);
  dom.window.close();
});
