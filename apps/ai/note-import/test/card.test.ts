import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Dictation, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Entry {
  id: string;
  date: string;
  title: string;
  chars: number;
  proposed: number;
  created: number;
}

interface FixtureProposal {
  id: string;
  type: string;
  name: string;
  excerpt: string;
  existing: string | null;
  checked: boolean;
}

/** The mock-up's fixtures: the draft a note is opened on, what the AI proposes, the imports already listed. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/NoteImport.json', import.meta.url), 'utf8')) as {
  campaign: string;
  draft: string;
  proposals: FixtureProposal[];
};
const imports = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/campaign.listImportedNotes.json', import.meta.url), 'utf8')) as Entry[];
const DUPLICATES = fixture.proposals.filter((p) => p.existing !== null);
const TICKED = fixture.proposals.filter((p) => p.existing === null);

interface Sent {
  dataCapability: string;
  payload: { campaign: string; text?: string; accepted?: string[] };
  caller: { by: string };
}

interface Applied {
  status: string;
  violations?: string[];
}

/** A shell over a small server: it answers the notes, and applies a command as the DataGuard would. */
function fakeShell(options: { apply?: (sent: Sent) => Applied; read?: () => unknown; fresh?: boolean } = {}) {
  const notes = { imports: structuredClone(imports), proposals: [] as FixtureProposal[] };
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables: Record<string, unknown> | undefined }> = [];
  const apply =
    options.apply ??
    ((command: Sent): Applied => {
      if (command.dataCapability === 'ai.extractFromNotes@1') notes.proposals = fixture.proposals.map((p) => ({ ...p, id: options.fresh ? `${p.id}#${sent.length}` : p.id, checked: p.existing === null }));
      if (command.dataCapability === 'campaign.createEntitiesFromNote@1') {
        const accepted = command.payload.accepted ?? [];
        notes.imports.unshift({ id: 'imp-new', date: '2026-10-18', title: 'Notes en vrac', chars: 100, proposed: notes.proposals.length, created: accepted.length });
        notes.proposals = [];
      }
      return { status: 'applied' };
    });
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      return { asOf: 1, data: options.read ? options.read() : structuredClone(notes) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = apply(command);
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: 'applied', violations: [] } };
    },
  };
  return { shell, sent, reads, notes };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the screen finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function screen(options?: Parameters<typeof fakeShell>[0], props: Record<string, unknown> = { campaign: fixture.campaign }, dictation?: Dictation) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="NoteImport"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing, dictation);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the screen`);
    return found;
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const tick = (id: string, on: boolean): void => {
    const box = $(`input[name="proposal"][value="${id}"]`) as HTMLInputElement;
    box.checked = on;
    box.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  };
  const field = (): HTMLTextAreaElement => $('#notes-text') as HTMLTextAreaElement;
  const ticked = (): string[] => Array.from(host.querySelectorAll<HTMLInputElement>('input[name="proposal"]:checked')).map((i) => i.value);
  return { document, host, mounted, $, click, tick, field, ticked, text: () => host.textContent ?? '', ...fake };
}

/** A screen with the draft typed and the AI's proposals on it. */
async function extracted(...args: Parameters<typeof screen>) {
  const s = await screen(...args);
  s.field().value = fixture.draft;
  await s.click('#notes-extract');
  return s;
}

test('the screen opens on the past imports, read by identifier alone, with how many proposals became elements', async () => {
  const s = await screen();
  assert.deepEqual(s.reads, [{ id: 'campaign.listImportedNotes', sha256: undefined, variables: { campaign: fixture.campaign } }]);
  const first = imports[0] as Entry;
  assert.match(s.$(`#notes-import-${first.id}`).textContent ?? '', new RegExp(`${first.title}.*${first.chars} caractères · ${first.created}/${first.proposed} créés`));
  assert.equal(s.$('#notes-history-list').children.length, imports.length);
  assert.equal(s.$('#notes-proposals').hidden, true);
});

test('every part names the unit it shows or triggers, by identifier', async () => {
  const s = await screen();
  assert.equal(s.$('#notes-extract').dataset.writes, 'ai.extractFromNotes');
  assert.equal(s.$('#notes-history-list').dataset.reads, 'campaign.listImportedNotes');
  assert.equal(s.$('#notes-proposals').dataset.reads, 'campaign.listImportedNotes');
  const sheet = await extracted();
  assert.equal(sheet.$('#notes-create').dataset.writes, 'campaign.createEntitiesFromNote');
});

test('an empty text area sends nothing and takes the focus', async () => {
  const s = await screen();
  s.field().value = '   \n ';
  await s.click('#notes-extract');
  assert.deepEqual(s.sent, []);
  assert.equal(s.document.activeElement, s.field());
});

test('extracting sends the note by identifier and shows the proposals, with nothing created yet', async () => {
  const s = await extracted();
  assert.equal(s.sent.length, 1);
  assert.deepEqual(s.sent[0], { dataCapability: 'ai.extractFromNotes@1', payload: { campaign: fixture.campaign, text: fixture.draft }, caller: { by: 'dm', from: 'NoteImport', roles: [] } });
  assert.equal(s.$('#notes-proposals').hidden, false);
  assert.equal(s.host.querySelectorAll('input[name="proposal"]').length, fixture.proposals.length);
  assert.equal(s.sent.some((c) => c.dataCapability.startsWith('campaign.createEntitiesFromNote')), false);
  assert.equal(s.field().value, fixture.draft, 'the raw note stays in the text area');
});

test('a proposal flagged as existing says so and starts unchecked; the others start checked', async () => {
  const s = await extracted();
  assert.ok(DUPLICATES.length > 0 && TICKED.length > 0);
  for (const p of DUPLICATES) {
    assert.equal((s.$(`input[name="proposal"][value="${p.id}"]`) as HTMLInputElement).checked, false, p.name);
    assert.match(s.$(`input[name="proposal"][value="${p.id}"]`).closest('label')?.textContent ?? '', /existe déjà/);
  }
  assert.deepEqual(s.ticked(), TICKED.map((p) => p.id));
  assert.equal(s.text().match(/existe déjà/g)?.length, DUPLICATES.length);
});

test('the create button counts the checked proposals and sends exactly their ids', async () => {
  const s = await extracted();
  assert.equal(s.$('#notes-create').textContent, `Créer la sélection (${TICKED.length})`);
  const ours = TICKED[0] as FixtureProposal;
  const dup = DUPLICATES[0] as FixtureProposal;
  s.tick(ours.id, false);
  s.tick(dup.id, true);
  assert.equal(s.$('#notes-create').textContent, `Créer la sélection (${TICKED.length})`);
  const expected = s.ticked();
  assert.ok(expected.includes(dup.id) && !expected.includes(ours.id));
  await s.click('#notes-create');
  assert.equal(s.sent.length, 2);
  assert.deepEqual(s.sent[1], { dataCapability: 'campaign.createEntitiesFromNote@1', payload: { campaign: fixture.campaign, accepted: expected }, caller: { by: 'dm', from: 'NoteImport', roles: [] } });
});

test('with nothing checked the create button is disabled and sends nothing', async () => {
  const s = await extracted();
  for (const id of s.ticked()) s.tick(id, false);
  const create = s.$('#notes-create') as HTMLButtonElement;
  assert.equal(create.disabled, true);
  assert.equal(create.textContent, 'Créer la sélection (0)');
  await s.click('#notes-create');
  assert.equal(s.sent.length, 1, 'only the extraction was sent');
});

test('once created, the proposals are gone, the import is listed and the raw note is said to be kept', async () => {
  const s = await extracted();
  await s.click('#notes-create');
  assert.equal(s.$('#notes-proposals').hidden, true);
  assert.match(s.$('#notes-done').textContent ?? '', new RegExp(`${TICKED.length} éléments créés · note brute conservée`));
  assert.equal(s.$('#notes-history-list').children.length, imports.length + 1);
  assert.equal(s.reads.length, 3, 'opened, extracted, created');
});

test('« Tout rejeter » closes the list and sends nothing; the rejected proposals stay out, a new extraction offers its own', async () => {
  const s = await extracted({ fresh: true });
  await s.click('#notes-cancel');
  assert.equal(s.$('#notes-proposals').hidden, true);
  assert.equal(s.sent.length, 1);
  await s.click('#notes-extract');
  assert.equal(s.sent.length, 2);
  assert.equal(s.$('#notes-proposals').hidden, false);
  assert.equal(s.host.querySelectorAll('input[name="proposal"]').length, fixture.proposals.length);
});

test('a refused extraction shows why and offers no proposals', async () => {
  const s = await extracted({ apply: () => ({ status: 'rejected', violations: ['text-only-payload'] }) });
  assert.match(s.$('#notes-error').textContent ?? '', /Extraction refusée : seul du texte est accepté/);
  assert.equal(s.$('#notes-proposals').hidden, true);
});

test('a note that yields nothing says so, the raw note being kept', async () => {
  const s = await extracted({ apply: () => ({ status: 'applied' }) });
  assert.match(s.text(), /Aucun élément trouvé dans cette note · note brute conservée/);
});

test('a screen with no campaign reads nothing and sends nothing', async () => {
  const s = await screen(undefined, {});
  assert.deepEqual(s.reads, []);
  s.field().value = fixture.draft;
  await s.click('#notes-extract');
  assert.deepEqual(s.sent, []);
  assert.match(s.$('#notes-error').textContent ?? '', /Aucune campagne/);
  s.mounted.update({ campaign: fixture.campaign });
  await settle();
  assert.equal(s.reads.length, 1, 'following its props, it reads the campaign it is given');
});

test('pending proposals of a note analysed earlier are offered when the screen opens', async () => {
  const s = await screen({ read: () => ({ imports, proposals: fixture.proposals }) });
  assert.equal(s.$('#notes-proposals').hidden, false);
  assert.deepEqual(s.ticked(), TICKED.map((p) => p.id));
});

test('when the dictation pauses, what was heard is appended to the text area', async () => {
  const s = await screen(undefined, { campaign: fixture.campaign }, { transcript: () => 'Mère Ombeline garde un herbier.' });
  s.field().value = 'Une première ligne.';
  await s.click('#notes-mic');
  assert.equal(s.$('#notes-mic-box').dataset.state, 'recording');
  assert.equal(s.field().value, 'Une première ligne.', 'nothing is appended while it records');
  await s.click('#notes-mic');
  assert.equal(s.$('#notes-mic-box').dataset.state, 'paused');
  assert.equal(s.field().value, 'Une première ligne.\nMère Ombeline garde un herbier.');
  await s.click('#notes-mic');
  assert.equal(s.$('#notes-mic-box').dataset.state, 'recording', 'a pause resumes');
  await s.click('#notes-mic');
  assert.equal(s.sent.length, 0, 'dictating sends nothing by itself');
});

test('a dictation into an empty text area starts it without a blank line, and a silent one adds nothing', async () => {
  const heard = await screen(undefined, { campaign: fixture.campaign }, { transcript: () => 'Dictée.' });
  await heard.click('#notes-mic');
  await heard.click('#notes-mic');
  assert.equal(heard.field().value, 'Dictée.');
  const silent = await screen();
  await silent.click('#notes-mic');
  await silent.click('#notes-mic');
  assert.equal(silent.field().value, '');
});

test('a new extraction offers its proposals even when their ids are ones the DM already settled', async () => {
  const s = await extracted();
  await s.click('#notes-create');
  assert.equal(s.$('#notes-proposals').hidden, true);
  await s.click('#notes-extract');
  assert.equal(s.$('#notes-proposals').hidden, false);
  assert.equal(s.host.querySelectorAll('input[name="proposal"]').length, fixture.proposals.length);
});
