import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Mounted, type Timing } from '../src/index.ts';
import type { Props, Shell } from '../src/types.ts';

interface Raw {
  v: number;
  date: string;
  origin: string;
  source: string;
  restoredFrom?: number | null;
  seen?: boolean;
  changes: Array<{ field: string; old: string; new: string }>;
}

/** The mock-up's fixture of the Capability: the histories a panel is opened on. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/campaign.listEntityVersions.json', import.meta.url), 'utf8')) as Record<string, { name: string; versions: Raw[] }>;

interface Sent {
  dataCapability: string;
  target: { aggregate: string; id: string };
  payload: { version: number; changes?: number[] };
  idempotencyKey: string;
}

interface Options {
  apply?: (sent: Sent, data: typeof fixture) => { status: string; violations?: string[] };
  read?: (entity: string) => unknown;
  later?: number;
}

/** A shell over a small server: it lists the histories, and applies a restore as the DataGuard would — a new version on top. */
function fakeShell(options: Options = {}) {
  const data = structuredClone(fixture);
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables: Record<string, unknown> | undefined }> = [];
  let listener: (() => void) | undefined;
  let asked = 0;
  const apply =
    options.apply ??
    ((command: Sent, store: typeof fixture) => {
      const history = store[command.target.id];
      const target = history?.versions.find((v) => v.v === command.payload.version);
      if (!history || !target) return { status: 'rejected', violations: ['not-found'] };
      history.versions.unshift({ v: (history.versions[0]?.v ?? 0) + 1, date: '2026-10-18T09:00:00', origin: 'manuelle', source: 'restauration', restoredFrom: target.v, changes: [{ field: 'Notes', old: 'avant', new: 'après' }] });
      return { status: 'applied' };
    });
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      const entity = String(variables?.entity);
      return { asOf: 1, data: options.read ? options.read(entity) : (structuredClone(data[entity]) ?? null) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = options.later === undefined ? apply(command, data) : { status: 'queued' };
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      asked += 1;
      const command = sent[Number(commandId.slice(1)) - 1] as Sent;
      const outcome = asked > (options.later ?? 0) ? apply(command, data) : { status: 'queued' };
      return { result: { commandId, status: outcome.status, violations: outcome.violations ?? [] } };
    },
    onDataVersion(next) {
      listener = () => next(1);
      return () => undefined;
    },
  };
  return { shell, sent, reads, data, moved: () => listener?.() };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the panel finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function panel(props: Props = { entity: 'morlox' }, options?: Options) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="VersionHistory"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted: Mounted = mount(host, props, fake.shell, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = document.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the page`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const ids = (): string[] => [...document.querySelectorAll('#versions-list > li')].map((li) => li.id);
  return { dom, document, host, mounted, $, click, ids, ...fake };
}

test('a sheet the AI changed offers « Mis à jour »; a seen one offers its history', async () => {
  const morlox = await panel({ entity: 'morlox' });
  assert.equal(morlox.$('#versions-updated').textContent, 'Mis à jour');
  assert.equal(morlox.document.querySelector('#versions-open'), null);
  assert.deepEqual(morlox.reads.map((r) => [r.id, r.sha256, r.variables]), [['campaign.listEntityVersions', undefined, { entity: 'morlox' }]]);
  const gaspard = await panel({ entity: 'gaspard' });
  assert.equal(gaspard.$('#versions-open').textContent, 'Historique des versions');
});

test('its parts name what they show and what they trigger', async () => {
  const p = await panel();
  assert.equal(p.$('#version-history').dataset.reads, 'campaign.listEntityVersions');
  await p.click('#versions-updated');
  assert.equal(p.$('#versions-diff').dataset.reads, 'campaign.listEntityVersions');
  assert.equal(p.$('#versions-list').dataset.reads, 'campaign.listEntityVersions');
  const writers = [...p.document.querySelectorAll<HTMLElement>('[data-writes]')];
  assert.deepEqual(writers.map((w) => w.dataset.action), ['restore:3', 'restore:2', 'restore:1', 'revert']);
  assert.ok(writers.every((w) => w.dataset.writes === 'campaign.restoreEntityVersion'));
});

test('the dialog opens on the unseen version’s diff, above the whole history newest first', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /Versions de « Morlox »/);
  assert.match(p.$('.kit-diffview__title').textContent ?? '', /^Version 4 — Brief oral — séance 4$/);
  assert.equal(p.document.querySelectorAll('#versions-diff .kit-diffview__row').length, 3);
  assert.match(p.$('#versions-diff .kit-diffview__old').textContent ?? '', /^Doit 40 po à Vesna\. Connaît le passage sous le moulin\.$/);
  assert.deepEqual(p.ids(), ['version-4', 'version-3', 'version-2', 'version-1']);
  assert.ok(p.$('#version-4').classList.contains('is-unseen'));
  assert.match(p.$('#version-4').textContent ?? '', /Non vue/);
  // The newest version is not restored onto itself; its changes can each be undone.
  assert.equal(p.document.querySelector('#version-4 [data-writes]'), null);
  assert.equal(p.document.querySelectorAll('#versions-diff .kit-diffview__check').length, 3);
  assert.equal((p.$('#versions-revert') as HTMLButtonElement).disabled, false);
  // An empty side of a diff is a dash.
  await p.click('#version-2 [data-action="show:2"]');
  assert.equal(p.document.querySelector('#versions-diff .kit-diffview__old')?.textContent, '—');
  // The creation has nothing before it: nothing to undo.
  await p.click('#version-1 [data-action="show:1"]');
  assert.equal(p.document.querySelector('#versions-diff .kit-diffview__check'), null);
  assert.equal((p.$('#versions-revert') as HTMLButtonElement).disabled, true);
  // Opening it, the DM has seen it.
  assert.equal(p.document.querySelector('#versions-updated'), null);
  assert.equal(p.document.querySelector('.mf-versions__item.is-unseen'), null);
  assert.ok(p.$('#versions-open'));
});

test('« Voir le diff » swaps the diff of the dialog, and Escape, the veil and « Fermer » close it', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  await p.click('#version-3 [data-action="show:3"]');
  assert.match(p.$('.kit-diffview__title').textContent ?? '', /^Version 3 — Fiche modifiée à la main après la séance 3$/);
  assert.equal(p.document.querySelectorAll('#versions-dialog').length, 1);
  p.document.dispatchEvent(new p.dom.window.KeyboardEvent('keydown', { key: 'Escape' }));
  assert.equal(p.document.querySelector('#versions-dialog'), null);
  await p.click('#versions-open');
  await p.click('#versions-close');
  assert.equal(p.document.querySelector('#versions-dialog'), null);
  await p.click('#versions-open');
  await p.click('.kit-dialog__veil');
  assert.equal(p.document.querySelector('#versions-dialog'), null);
});

test('restoring version 2 of morlox sends one exact command, and the restoration tops the history (restore-exact)', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  await p.click('#version-2 [data-writes]');
  assert.deepEqual(
    p.sent.map(({ idempotencyKey, ...command }) => ({ ...command, keyed: idempotencyKey.startsWith('VersionHistory:morlox:2:') })),
    [
      {
        dataCapability: 'campaign.restoreEntityVersion@1',
        target: { aggregate: 'Entity', id: 'morlox' },
        payload: { version: 2 },
        caller: { by: 'dm', from: 'VersionHistory', roles: [] },
        keyed: true,
      },
    ],
  );
  assert.deepEqual(p.ids(), ['version-5', 'version-4', 'version-3', 'version-2', 'version-1']);
  assert.match(p.$('#version-5').textContent ?? '', /Retour à la v2/);
  assert.match(p.$('.kit-diffview__title').textContent ?? '', /^Version 5 — Retour à la v2$/);
  assert.match(p.$('#versions-status').textContent ?? '', /La v2 est restaurée : la v5 est en tête/);
  assert.ok(p.$('#versions-status').classList.contains('is-ok'));
  // The restoration is undone like any version: it has no restore of its own on top, the one before it does.
  assert.equal(p.document.querySelector('#version-5 [data-writes]'), null);
  assert.ok(p.$('#version-4 [data-writes]'));
  // Versions 1 to 4 are as they were.
  assert.deepEqual(p.data.morlox?.versions.slice(1), fixture.morlox?.versions);
  // The restore was the DM’s: nothing is « Mis à jour ».
  assert.ok(p.$('#versions-open'));
});

test('a restore can be followed until the server applies it', async () => {
  const p = await panel({ entity: 'morlox' }, { later: 2 });
  await p.click('#versions-updated');
  await p.click('#version-3 [data-writes]');
  assert.equal(p.sent.length, 1);
  assert.deepEqual(p.ids().slice(0, 2), ['version-5', 'version-4']);
});

test('a restore the DataGuard refuses is said, and the history is left as it was', async () => {
  const p = await panel({ entity: 'morlox' }, { apply: () => ({ status: 'rejected', violations: ['alias-unique-in-campaign'] }) });
  await p.click('#versions-updated');
  const opened = p.reads.length;
  await p.click('#version-2 [data-writes]');
  assert.match(p.$('#versions-status').textContent ?? '', /^Restauration refusée : un alias de cette version appartient déjà à un autre élément\.$/);
  assert.ok(p.$('#versions-status').classList.contains('is-error'));
  assert.deepEqual(p.ids(), ['version-4', 'version-3', 'version-2', 'version-1']);
  assert.equal(p.reads.length, opened, 'nothing was applied: nothing is read again');
  // The buttons are free again.
  assert.equal((p.$('#version-2 [data-writes]') as HTMLButtonElement).disabled, false);
});

test('a restore that never settles, and one that cannot be sent, say so', async () => {
  const stuck = await panel({ entity: 'morlox' }, { later: 100 });
  await stuck.click('#versions-updated');
  await stuck.click('#version-2 [data-writes]');
  assert.match(stuck.$('#versions-status').textContent ?? '', /n’a pas abouti \(état : queued\)/);
  const down = await panel({ entity: 'morlox' }, { apply: () => { throw new Error('down'); } });
  await down.click('#versions-updated');
  await down.click('#version-2 [data-writes]');
  assert.equal(down.$('#versions-status').textContent, 'La restauration a échoué.');
});

test('a restore applied whose history cannot be read again is said as applied', async () => {
  let reads = 0;
  const p = await panel({ entity: 'morlox' }, { read: (entity) => { reads += 1; if (reads > 1) throw new Error('down'); return structuredClone(fixture[entity]); } });
  await p.click('#versions-updated');
  await p.click('#version-2 [data-writes]');
  assert.match(p.$('#versions-status').textContent ?? '', /La v2 est restaurée, mais l’historique n’a pas pu être relu/);
});

test('a double click restores once', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  const button = p.$('#version-2 [data-writes]');
  button.dispatchEvent(new p.dom.window.MouseEvent('click', { bubbles: true }));
  button.dispatchEvent(new p.dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  assert.equal(p.sent.length, 1);
});

test('a sheet changed by someone else while it is on screen becomes « Mis à jour »', async () => {
  const p = await panel({ entity: 'gaspard' });
  // The first word of the shell only says where the data stands.
  p.moved();
  await settle();
  assert.equal(p.reads.length, 1);
  assert.ok(p.$('#versions-open'));
  p.data.gaspard?.versions.unshift({ v: 3, date: '2026-10-18T08:00:00', origin: 'ia', source: 'ia', restoredFrom: null, changes: [{ field: 'Notes', old: '', new: 'nouvelle' }] });
  p.moved();
  await settle();
  assert.equal(p.$('#versions-updated').textContent, 'Mis à jour');
  await p.click('#versions-updated');
  assert.match(p.$('.kit-diffview__title').textContent ?? '', /^Version 3 — Génération par l’IA$/);
  // A change of the DM's own is not news.
  p.data.gaspard?.versions.unshift({ v: 4, date: '2026-10-18T08:30:00', origin: 'manuelle', source: 'dm', restoredFrom: null, changes: [{ field: 'Notes', old: 'nouvelle', new: 'à la main' }] });
  p.moved();
  await settle();
  assert.ok(p.$('#versions-open'));
  assert.deepEqual(p.ids().slice(0, 2), ['version-4', 'version-3']);
});

test('another sheet is another history; an unknown one has none', async () => {
  const p = await panel({ entity: 'morlox' });
  p.mounted.update({ entity: 'elowen' });
  await settle();
  assert.deepEqual(p.reads.map((r) => r.variables?.entity), ['morlox', 'elowen']);
  await p.click('#versions-open');
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /Elowen Ventargent/);
  assert.equal(p.document.querySelectorAll('#versions-list > li').length, 3);
  assert.match(p.$('#version-3').textContent ?? '', /D&amp;D Beyond|D&D Beyond/);
  await p.click('#versions-close');
  p.mounted.update({ entity: 'nouvelle-fiche' });
  await settle();
  await p.click('#versions-open');
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /Nouvelle fiche/);
  assert.equal(p.$('#versions-diff').textContent, 'Aucune version.');
  assert.equal(p.document.querySelectorAll('#versions-list > li').length, 0);
});

test('no sheet, no read', async () => {
  const p = await panel({});
  assert.equal(p.reads.length, 0);
  assert.ok(p.$('#versions-open'));
});

test('open waits for the sheet’s own history', async () => {
  const p = await panel({ entity: 'gaspard' });
  p.mounted.update({ entity: 'morlox' });
  await p.mounted.open();
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /Morlox/);
});

test('a titled panel names the sheet the last brief changed; unreadable, it says so', async () => {
  const p = await panel({ entity: 'morlox', title: 'Fiches modifiées par le brief' });
  assert.ok(p.$('#version-history').classList.contains('is-panel'));
  assert.equal(p.$('#versions-title').textContent, 'Fiches modifiées par le brief');
  assert.equal(p.$('#versions-head').hidden, false);
  assert.ok(p.$('#versions-head').contains(p.$('#versions-updated')));
  assert.match(p.$('#versions-note').textContent ?? '', /^Morlox a été mise à jour : Brief oral — séance 4\. Ouvrez l’historique/);
  const empty = await panel({ entity: 'nouvelle-fiche', title: 'Fiches' });
  assert.equal(empty.$('#versions-note').hidden, true);
  const plain = await panel({ entity: 'morlox' });
  assert.equal(plain.$('#versions-head').hidden, true);
  assert.equal(plain.$('#versions-note').hidden, true);
  const failing = await panel({ entity: 'morlox' }, { read: () => { throw new Error('down'); } });
  assert.equal(failing.$('#versions-note').hidden, false);
  assert.match(failing.$('#versions-note').textContent ?? '', /Impossible de lire l’historique/);
  assert.ok(failing.$('#versions-open'));
});

test('what the history holds is shown as text, never as markup', async () => {
  const p = await panel({ entity: 'x' }, { read: () => ({ name: '<b>Gob</b>', versions: [{ v: 1, date: '2026-10-18T08:30:00', origin: 'ia', source: '<img src=x onerror=alert(1)>', restoredFrom: null, changes: [{ field: '<i>f</i>', old: '', new: '<script>1</script>' }] }] }) });
  await p.click('#versions-open');
  assert.equal(p.document.querySelector('#versions-dialog img, #versions-dialog script, #versions-dialog i'), null);
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /<b>Gob<\/b>/);
});

test('undoing ticked changes sends the version before the diff on show and the ticked positions', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  // Nothing ticked: nothing sent.
  await p.click('#versions-revert');
  assert.equal(p.sent.length, 0);
  assert.equal(p.$('#versions-status').textContent, 'Cochez les changements à annuler.');
  const boxes = p.document.querySelectorAll<HTMLInputElement>('#versions-diff .kit-diffview__check');
  for (const index of [0, 2]) {
    const box = boxes[index] as HTMLInputElement;
    box.checked = true;
    box.dispatchEvent(new p.dom.window.Event('change', { bubbles: true }));
  }
  assert.equal(p.document.querySelectorAll('#versions-diff .kit-diffview__row--reverted').length, 2);
  await p.click('#versions-revert');
  assert.equal(p.sent.length, 1);
  assert.deepEqual(p.sent[0]?.payload, { version: 3, changes: [0, 2] });
  assert.deepEqual(p.sent[0]?.target, { aggregate: 'Entity', id: 'morlox' });
  assert.deepEqual(p.ids().slice(0, 2), ['version-5', 'version-4']);
  assert.match(p.$('#versions-status').textContent ?? '', /^2 changement\(s\) de la v4 annulé\(s\) : la v5 est en tête/);
});

test('a history that moves under an open dialog keeps the diff the DM chose and what the dialog said', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  await p.click('#version-2 [data-action="show:2"]');
  await p.click('#version-3 [data-writes]');
  assert.match(p.$('#versions-status').textContent ?? '', /est restaurée/);
  await p.click('#version-2 [data-action="show:2"]');
  // Someone else writes: the dialog follows the history, not the newest diff.
  p.data.morlox?.versions.unshift({ v: 6, date: '2026-10-18T10:00:00', origin: 'ia', source: 'ia', restoredFrom: null, changes: [{ field: 'Alias', old: 'a', new: 'b' }] });
  p.moved();
  await settle();
  p.moved();
  await settle();
  assert.equal(p.ids()[0], 'version-6');
  assert.match(p.$('.kit-diffview__title').textContent ?? '', /^Version 2 /);
  // Said before the DM moved on to another diff, it was cleared by that move.
  await p.click('#version-3 [data-writes]');
  assert.match(p.$('#versions-status').textContent ?? '', /est restaurée/);
  p.moved();
  await settle();
  assert.match(p.$('#versions-status').textContent ?? '', /est restaurée/);
});

test('another sheet closes the dialog of the previous one and shows nothing of it', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  p.mounted.update({ entity: 'elowen' });
  assert.equal(p.document.querySelector('#versions-dialog'), null);
  assert.ok(p.$('#versions-open'));
  assert.equal(p.document.querySelector('#versions-updated'), null);
  await settle();
});

test('a click on the trigger while the history is being read opens it once it is', async () => {
  const p = await panel({ entity: 'gaspard' });
  p.mounted.update({ entity: 'morlox' });
  await p.click('#versions-open');
  assert.match(p.$('.kit-dialog__title').textContent ?? '', /Morlox/);
});

// AC1: a first read done, the server gains a version, the trigger is clicked: it reads again and the dialog lists the new version
test('the trigger reads the history again, and the dialog opens on what the server holds now', async () => {
  const p = await panel({ entity: 'gaspard' });
  const before = p.reads.length;
  // The server gains a version the shell has not told the panel about yet (its poll is 2 s).
  p.data.gaspard?.versions.unshift({ v: 9, date: '2026-10-18T11:00:00', origin: 'manuelle', source: 'sauvegarde', restoredFrom: null, changes: [{ field: 'Notes', old: 'a', new: 'b' }] });
  await p.click('#versions-open');
  assert.equal(p.reads.length, before + 1);
  assert.deepEqual(p.reads.at(-1)?.variables, { entity: 'gaspard' });
  assert.equal(p.ids()[0], 'version-9');
});

// AC1: the programmatic open reads again as well
test('open() reads the history again before it opens', async () => {
  const p = await panel({ entity: 'gaspard' });
  p.data.gaspard?.versions.unshift({ v: 9, date: '2026-10-18T11:00:00', origin: 'manuelle', source: 'sauvegarde', restoredFrom: null, changes: [{ field: 'Notes', old: 'a', new: 'b' }] });
  await p.mounted.open();
  assert.equal(p.ids()[0], 'version-9');
});

// AC2: the trigger clicked while a read of another element is in flight, the old answer is dropped
test('the answer of an element left behind is dropped, whatever the click', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="VersionHistory"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell();
  const answers = new Map<string, (data: unknown) => void>();
  let hold = false;
  const shell: Shell = {
    ...fake.shell,
    capability(id, sha256, variables) {
      const entity = String(variables?.entity);
      if (!hold || entity !== 'morlox') return fake.shell.capability(id, sha256, variables);
      return new Promise((resolve) => answers.set(entity, (data) => resolve({ asOf: 1, data })));
    },
  };
  const mounted = mount(host, { entity: 'gaspard' }, shell, timing);
  await settle();
  // Morlox's read is in flight when the trigger of the panel (now Morlox's) is clicked.
  hold = true;
  mounted.update({ entity: 'morlox' });
  hold = false;
  host.querySelector<HTMLElement>('#versions-open')?.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
  await settle();
  // Another element takes the panel before the answers arrive.
  mounted.update({ entity: 'elowen' });
  await settle();
  answers.get('morlox')?.(structuredClone(fixture.morlox));
  await settle();
  assert.equal(document.querySelector('#versions-dialog'), null);
  assert.equal(document.querySelector('#version-4'), null);
  assert.match(host.querySelector('#versions-trigger')?.textContent ?? '', /Historique des versions/);
});

test('a panel taken off the page lets go of its dialog on the shell’s next word', async () => {
  const p = await panel();
  await p.click('#versions-updated');
  p.host.remove();
  p.moved();
  assert.equal(p.document.querySelector('#versions-dialog'), null);
});

test('a refusal with no reason is still a refusal', async () => {
  const p = await panel({ entity: 'morlox' }, { apply: () => ({ status: 'rejected' }) });
  await p.click('#versions-updated');
  await p.click('#version-2 [data-writes]');
  assert.equal(p.$('#versions-status').textContent, 'Restauration refusée.');
});
