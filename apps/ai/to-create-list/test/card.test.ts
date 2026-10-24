import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Dictation, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Entry {
  id: string;
  type: string;
  name: string;
  session: string;
  sessionTitle: string;
  excerpt: string;
  status: 'todo' | 'created';
  entity: string | null;
}

/** The mock-up's fixtures: the entries the Capability answers, and what the AI proposes for them. */
const entries = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/campaign.listEntitiesToCreate.json', import.meta.url), 'utf8')) as Entry[];
const proposals = (JSON.parse(readFileSync(new URL('../../../../mock/fixtures/ToCreateList.json', import.meta.url), 'utf8')) as { proposals: Record<string, { dictation: string; fromBrief: Record<string, unknown>; refined: Record<string, unknown> }> }).proposals;
const ODRIC = entries.find((e) => e.id === 'tc-odric') as Entry;
const GUE = entries.find((e) => e.id === 'tc-gue') as Entry;

interface Sent {
  dataCapability: string;
  payload: { entry: string; refinement?: string };
  target?: { aggregate: string; id: string };
}

interface Applied {
  status: string;
  violations?: string[];
}

/** A shell over a small server: it answers the list and the element, and applies the creation as the DataGuard would. */
function fakeShell(options: { apply?: (sent: Sent) => Applied; entity?: () => unknown; hold?: Promise<void> } = {}) {
  const list = structuredClone(entries);
  const made = new Map<string, unknown>();
  const sent: Sent[] = [];
  const reads: Array<{ id: string; sha256: string | undefined; variables: Record<string, unknown> | undefined }> = [];
  const apply =
    options.apply ??
    ((command: Sent): Applied => {
      const entry = list.find((e) => e.id === command.payload.entry) as Entry;
      const given = proposals[entry.id];
      const proposal = (command.payload.refinement === undefined ? given?.fromBrief : given?.refined) ?? { description: entry.excerpt };
      entry.status = 'created';
      entry.entity = `new-${entry.id}`;
      const statblock = proposal.statblock as { vitals: Array<{ label: string; value: string }>; abilities: Record<string, number>; features: unknown[] } | undefined;
      made.set(entry.entity, {
        id: entry.entity,
        name: entry.name,
        aliases: [],
        ...proposal,
        ...(statblock === undefined
          ? {}
          : { stats: statblock.abilities, ca: Number(statblock.vitals[0]?.value), pvMax: 13, features: statblock.features, modifiers: { FOR: 1, DEX: -1, CON: 0, INT: 0, SAG: 2, CHA: 0 } }),
      });
      return { status: 'applied' };
    });
  const shell: Shell = {
    async capability(id, sha256, variables) {
      reads.push({ id, sha256, variables });
      if (id === 'campaign.getEntity') return { asOf: 1, data: options.entity ? options.entity() : made.get(String(variables?.id)) };
      return { asOf: 1, data: { items: structuredClone(list) } };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      await options.hold;
      const outcome = apply(command);
      return { commandId: `c${sent.length}`, result: { status: outcome.status, violations: outcome.violations ?? [] } };
    },
    async commandResult(commandId) {
      return { result: { commandId, status: 'applied', violations: [] } };
    },
  };
  return { shell, sent, reads };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the screen finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function screen(options?: Parameters<typeof fakeShell>[0], props: Record<string, unknown> = { campaign: 'cendres-valombre' }, dictation?: Dictation) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="ToCreateList"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const mounted = mount(host, props, fake.shell, timing, dictation);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the screen`);
    return found;
  };
  const button = (entry: Entry, label: string): HTMLElement => {
    const found = Array.from($(`#to-create-${entry.id}`).querySelectorAll('button')).find((b) => b.textContent === label);
    assert.ok(found, `« ${label} » is on ${entry.id}`);
    return found;
  };
  const click = async (target: HTMLElement): Promise<void> => {
    target.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const write = (entry: Entry, value: string): void => {
    const field = $(`#to-create-${entry.id}-text`) as HTMLTextAreaElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  const item = (entry: Entry): HTMLElement => $(`#to-create-${entry.id}`);
  return { host, mounted, $, button, click, write, item, count: () => $('#to-create-count').textContent, ...fake };
}

test('each entry is a list item: its name as heading, its type, its session, its excerpt, and its state in a badge', async () => {
  const s = await screen();
  assert.equal(s.$('#to-create-items').children.length, entries.length);
  for (const entry of entries) {
    const text = s.item(entry).textContent ?? '';
    assert.equal(s.item(entry).querySelector('h3')?.textContent, entry.name);
    assert.ok(text.includes(entry.sessionTitle) && text.includes(entry.excerpt));
    assert.ok(text.includes(entry.status === 'created' ? 'Créé' : 'À créer'));
  }
  assert.equal(s.count(), '2 à créer · 1 créée(s)');
  assert.deepEqual(s.reads, [{ id: 'campaign.listEntitiesToCreate', sha256: undefined, variables: { campaign: 'cendres-valombre' } }]);
});

test('every part names the unit it shows or triggers, by identifier', async () => {
  const s = await screen();
  assert.equal(s.$('#to-create-items').dataset.reads, 'campaign.listEntitiesToCreate');
  assert.equal(s.button(ODRIC, 'Créer depuis le brief').dataset.writes, 'ai.createFromBrief');
  await s.click(s.button(ODRIC, 'Affiner'));
  assert.equal(s.button(ODRIC, 'Proposer').dataset.writes, 'ai.createFromBrief');
  assert.equal(s.item(ODRIC).querySelector('button[data-action="refine"]'), null, 'Affiner writes nothing');
});

test('« Créer depuis le brief » sends the entry with no refinement, and the entry reads « Créé »', async () => {
  const s = await screen();
  await s.click(s.button(ODRIC, 'Créer depuis le brief'));
  assert.deepEqual(s.sent, [{ dataCapability: 'ai.createFromBrief@1', payload: { entry: 'tc-odric' }, target: { aggregate: 'Session', id: 's4' }, caller: { by: 'dm', from: 'ToCreateList', roles: [] } }]);
  assert.ok(s.item(ODRIC).textContent?.includes('Créé'));
  assert.equal(s.item(ODRIC).textContent?.includes('À créer'), false);
  assert.equal(s.count(), '1 à créer · 2 créée(s)');
  assert.ok(s.item(ODRIC).querySelector('a[href="fiche-entite.html?id=new-tc-odric"]'));
});

test('« Affiner » opens « Description affinée »; « Proposer » sends the sentences, and the proposal shows its background and its armour class', async () => {
  const s = await screen();
  await s.click(s.button(ODRIC, 'Affiner'));
  assert.equal(s.item(ODRIC).querySelector('label')?.textContent, 'Description affinée');
  s.write(ODRIC, (proposals['tc-odric'] as { dictation: string }).dictation);
  await s.click(s.button(ODRIC, 'Proposer'));
  assert.equal(s.sent.length, 1);
  assert.equal(s.sent[0]?.payload.refinement, (proposals['tc-odric'] as { dictation: string }).dictation);
  const dl = s.item(ODRIC).querySelectorAll('dl');
  assert.equal(dl.length, 1, 'one description list, for the proposal');
  const text = dl[0]?.textContent ?? '';
  assert.ok(text.includes('Background') && text.includes('Vieil homme qui boite') && text.includes('Odric a vu les cavaliers'));
  const statblock = s.item(ODRIC).querySelector('.kit-statblock')?.textContent ?? '';
  assert.match(statblock, /CA\s*10/);
  assert.match(statblock, /DEX\s*8\s*−1/);
  assert.equal(s.count(), '1 à créer · 2 créée(s)');
  assert.equal(s.reads.some((r) => r.id === 'campaign.getEntity' && r.variables?.id === 'new-tc-odric'), true);
});

test('a place has its description and aliases, and no statblock', async () => {
  const s = await screen();
  await s.click(s.button(GUE, 'Créer depuis le brief'));
  assert.ok(s.item(GUE).querySelector('dl')?.textContent?.includes('Alias proposés'));
  assert.equal(s.item(GUE).querySelector('.kit-statblock'), null);
});

test('a blank refinement is no refinement, and « Annuler » sends nothing', async () => {
  const s = await screen();
  await s.click(s.button(ODRIC, 'Affiner'));
  s.write(ODRIC, '   ');
  await s.click(s.button(ODRIC, 'Proposer'));
  assert.equal('refinement' in (s.sent[0]?.payload ?? {}), false);

  const other = await screen();
  await other.click(other.button(GUE, 'Affiner'));
  other.write(GUE, 'Un rocher fendu.');
  await other.click(other.button(GUE, 'Annuler'));
  assert.deepEqual(other.sent, []);
  assert.ok(other.button(GUE, 'Affiner'));
  await other.click(other.button(GUE, 'Affiner'));
  assert.equal((other.item(GUE).querySelector('textarea') as HTMLTextAreaElement).value, '', 'a cancelled sentence is not kept');
});

test('what is typed survives the repaint of another entry', async () => {
  const s = await screen();
  await s.click(s.button(ODRIC, 'Affiner'));
  s.write(ODRIC, 'Il boite.');
  await s.click(s.button(GUE, 'Affiner'));
  assert.equal((s.item(ODRIC).querySelector('textarea') as HTMLTextAreaElement).value, 'Il boite.');
});

test('a refused creation says why and leaves the entry to create', async () => {
  const s = await screen({ apply: () => ({ status: 'rejected', violations: ['to-create-leaves-once-created'] }) });
  await s.click(s.button(ODRIC, 'Créer depuis le brief'));
  assert.match(s.item(ODRIC).textContent ?? '', /Création refusée : cet élément est déjà créé\./);
  assert.ok(s.button(ODRIC, 'Créer depuis le brief'));
  assert.equal(s.count(), '2 à créer · 1 créée(s)');
});

test('a created element that cannot be read is still created, without a proposal', async () => {
  const s = await screen({ entity: () => null });
  await s.click(s.button(ODRIC, 'Créer depuis le brief'));
  assert.ok(s.item(ODRIC).textContent?.includes('Créé'));
  assert.equal(s.item(ODRIC).querySelector('dl'), null);
});

test('a session in the props keeps its own entries', async () => {
  const s = await screen(undefined, { campaign: 'cendres-valombre', session: 's9' });
  assert.equal(s.$('#to-create-items').children.length, 0);
  assert.equal(s.count(), '0 à créer · 0 créée(s)');
  s.mounted.update({ campaign: 'cendres-valombre', session: 's4' });
  assert.equal(s.$('#to-create-items').children.length, entries.length);
});

test('a screen with no campaign reads nothing', async () => {
  const s = await screen(undefined, {});
  assert.deepEqual(s.reads, []);
});

test('the microphone starts a dictation, and its pause hands the transcript to the field', async () => {
  const heard = 'Dicté : il boite.';
  const s = await screen(undefined, { campaign: 'cendres-valombre' }, { transcript: () => heard });
  await s.click(s.button(ODRIC, 'Affiner'));
  const mic = s.item(ODRIC).querySelector<HTMLElement>('.kit-micbutton__btn') as HTMLElement;
  await s.click(mic);
  assert.equal(s.item(ODRIC).querySelector('.kit-micbutton')?.getAttribute('data-state'), 'recording');
  await s.click(s.item(ODRIC).querySelector<HTMLElement>('.kit-micbutton__btn') as HTMLElement);
  assert.equal(s.item(ODRIC).querySelector('.kit-micbutton')?.getAttribute('data-state'), 'paused');
  assert.equal((s.item(ODRIC).querySelector('textarea') as HTMLTextAreaElement).value, heard);
});

test('while a creation runs the screen holds still: one command at a time', async () => {
  let release = (): void => undefined;
  const s = await screen({ hold: new Promise<void>((resolve) => (release = resolve)) });
  await s.click(s.button(ODRIC, 'Créer depuis le brief'));
  assert.match(s.item(ODRIC).textContent ?? '', /Création depuis le brief…/);
  assert.equal(s.button(GUE, 'Créer depuis le brief').hasAttribute('disabled'), true);
  // A click that reaches the other button all the same is ignored.
  s.button(GUE, 'Créer depuis le brief').removeAttribute('disabled');
  await s.click(s.button(GUE, 'Créer depuis le brief'));
  assert.equal(s.sent.length, 1);
  release();
  await new Promise((resolve) => setImmediate(resolve));
});
