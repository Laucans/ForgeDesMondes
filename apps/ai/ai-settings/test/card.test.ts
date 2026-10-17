import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Provider {
  url: string;
  generationModel: string;
  summaryModel: string;
}

/** The mock-up's fixture of the Capability: the settings a card is opened on. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/ai.getSettings.json', import.meta.url), 'utf8')) as {
  providers: { cloud: Provider; opencode: Provider };
  lastTest: unknown;
};
const HOME = fixture.providers.opencode.url;

interface Applied {
  status: string;
  violations?: string[];
}

interface Sent {
  dataCapability: string;
  payload: Record<string, string>;
  caller: { by: string; roles: string[] };
}

/** A shell over a small server: it answers the settings, and applies a command as the DataGuard would. */
function fakeShell(options: { read?: () => unknown; apply?: (sent: Sent) => Applied; later?: number; gate?: Promise<void> } = {}) {
  const data = structuredClone(fixture);
  const sent: Sent[] = [];
  const reads: Array<string | undefined> = [];
  let asked = 0;
  const apply =
    options.apply ??
    ((command: Sent): Applied => {
      if (command.dataCapability === 'ai.testConnection@1') {
        const known = command.payload.mode === 'binary' || command.payload.url === HOME;
        data.lastTest = known
          ? { ok: true, at: '2026-10-17T10:00:00Z', provider: command.payload.mode === 'binary' ? command.payload.binary : 'opencode', latency: '0,84 s', message: 'Connexion réussie (fake) — génération : g, résumé : s' }
          : { ok: false, at: '2026-10-17T10:00:00Z', provider: 'opencode', latency: null, message: "type d'API non reconnu" };
      }
      return { status: 'applied' };
    });
  const shell: Shell = {
    async capability(id, sha256) {
      reads.push(id);
      await options.gate;
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      return { asOf: 1, data: options.read ? options.read() : structuredClone(data) };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      const outcome = apply(command);
      const status = options.later === undefined ? outcome.status : 'queued';
      return { commandId: `c${sent.length}`, result: { status, violations: outcome.violations ?? [] }, _outcome: outcome };
    },
    async commandResult(commandId) {
      asked += 1;
      const outcome = apply(sent[Number(commandId.slice(1)) - 1] as Sent);
      return { result: { commandId, status: asked > (options.later ?? 0) ? outcome.status : 'queued', violations: outcome.violations ?? [] } };
    },
  };
  return { shell, sent, reads, data };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

async function card(options?: Parameters<typeof fakeShell>[0], waiting: Timing = timing, ready = true) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="AiSettings"></div></body></html>');
  const { document } = dom.window;
  const host = document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  mount(host, {}, fake.shell, waiting);
  if (ready) await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the card`);
    return found;
  };
  const type = (selector: string, value: string): void => {
    const field = $(selector) as HTMLInputElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  const choose = (selector: string, value: string): void => {
    const field = $(selector) as HTMLInputElement | HTMLSelectElement;
    if (field instanceof dom.window.HTMLInputElement) field.checked = true;
    else field.value = value;
    field.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
  };
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const text = (): string => host.textContent ?? '';
  return { host, $, type, choose, click, text, ...fake };
}

/** Lets every pending promise of the card finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

test('the reminder is on the card in any state, even when the settings cannot be read', async () => {
  const ok = await card();
  assert.match(ok.text(), /Seul du texte part vers le fournisseur/);
  const failing = await card({ read: () => { throw new Error('down'); } });
  assert.match(failing.text(), /Seul du texte part vers le fournisseur/);
  assert.match(failing.$('#ai-test-result').textContent ?? '', /Impossible de lire/);
});

test('the card opens on the saved provider, with what stays local and what is sent', async () => {
  const c = await card();
  assert.equal((c.$('#ai-url') as HTMLInputElement).value, HOME);
  assert.equal((c.$('#ai-generation-model') as HTMLInputElement).value, fixture.providers.opencode.generationModel);
  assert.match(c.$('#ai-detected').textContent ?? '', /Type d’API détecté : Endpoint opencode du serveur maison/);
  assert.match(c.$('#ai-test-result').textContent ?? '', /^Dernier test réussi le .* — réponse en 0,84 s\.$/);
  assert.equal(c.$('#ai-local-list').children.length, 3);
  assert.equal(c.$('#ai-sent-list').children.length, 3);
  assert.deepEqual(c.reads, ['ai.getSettings']);
});

test('API mode asks for no API type, and binary mode offers exactly opencode and Claude Code', async () => {
  const c = await card();
  assert.equal(c.host.querySelectorAll('input[name="ai-mode"]').length, 2);
  assert.doesNotMatch([...c.host.querySelectorAll('label')].map((l) => l.firstChild?.textContent).join('|'), /Type d’API/);
  assert.equal(c.host.querySelector('select'), null);
  c.choose('input[name="ai-mode"][value="binary"]', 'binary');
  const options = [...c.$('#ai-binary').querySelectorAll('option')].map((o) => o.textContent);
  assert.deepEqual(options, ['opencode', 'Claude Code']);
  assert.equal(c.host.querySelector('#ai-url'), null);
  assert.equal(c.host.querySelector('#ai-key'), null);
  assert.equal(c.$('#ai-detected').textContent, '');
  assert.match(c.text(), /Seul du texte part vers le fournisseur/);
});

test('picking Claude Code brings its models', async () => {
  const c = await card();
  c.choose('input[name="ai-mode"][value="binary"]', 'binary');
  c.choose('#ai-binary', 'claude');
  assert.equal((c.$('#ai-generation-model') as HTMLInputElement).value, fixture.providers.cloud.generationModel);
});

test('the test button sends ai.testConnection alone, never a save, and shows the result and the detected type', async () => {
  const c = await card();
  await c.click('#ai-test');
  assert.deepEqual(c.sent.map((s) => s.dataCapability), ['ai.testConnection@1']);
  assert.deepEqual(c.sent[0]?.payload, { mode: 'api', url: HOME, key: '', generationModel: fixture.providers.opencode.generationModel, summaryModel: fixture.providers.opencode.summaryModel });
  assert.deepEqual(c.sent[0]?.caller.roles, []);
  assert.match(c.$('#ai-test-message').textContent ?? '', /^Connexion réussie/);
  assert.ok(c.$('#ai-test-message').classList.contains('is-ok'));
  assert.match(c.$('#ai-detected-badge').textContent ?? '', /Type d’API détecté : Endpoint opencode du serveur maison/);
});

test('an unrecognised URL shows the reason, no success, and the URL is not what a reload finds', async () => {
  const c = await card();
  c.type('#ai-url', 'https://inconnu.example.org/pas-une-api');
  assert.equal(c.$('#ai-detected').textContent, '', 'the detected type is not the new URL’s');
  await c.click('#ai-test');
  const message = c.$('#ai-test-message');
  assert.match(message.textContent ?? '', /reconnu/);
  assert.ok(message.classList.contains('is-error'));
  assert.doesNotMatch(c.text(), /Connexion réussie/);
  assert.equal(c.sent.some((s) => s.dataCapability === 'ai.saveSettings@1'), false);
  const reloaded = await card({ read: () => c.data });
  assert.equal((reloaded.$('#ai-url') as HTMLInputElement).value, HOME);
});

test('a binary is tested without a URL', async () => {
  const c = await card();
  c.choose('input[name="ai-mode"][value="binary"]', 'binary');
  c.choose('#ai-binary', 'claude');
  await c.click('#ai-test');
  assert.deepEqual(c.sent[0]?.payload.binary, 'claude');
  assert.equal(c.sent[0]?.payload.url, undefined);
  assert.match(c.$('#ai-test-message').textContent ?? '', /^Connexion réussie/);
});

test('the save button sends ai.saveSettings alone and the clear key leaves the screen', async () => {
  const c = await card();
  c.type('#ai-key', 'sk-secret');
  await c.click('#ai-save');
  assert.deepEqual(c.sent.map((s) => s.dataCapability), ['ai.saveSettings@1']);
  assert.equal(c.sent[0]?.payload.key, 'sk-secret');
  assert.match(c.$('#ai-test-message').textContent ?? '', /Paramètres enregistrés/);
  assert.equal((c.$('#ai-key') as HTMLInputElement).value, '');
  assert.doesNotMatch(c.host.innerHTML, /sk-secret/);
});

test('a form that cannot be sent sends nothing and says why', async () => {
  const c = await card();
  c.type('#ai-generation-model', '  ');
  await c.click('#ai-test');
  await c.click('#ai-save');
  assert.deepEqual(c.sent, []);
  assert.match(c.$('#ai-test-message').textContent ?? '', /modèle de génération est obligatoire/);
});

test('a refused command says its reasons', async () => {
  const c = await card({ apply: () => ({ status: 'rejected', violations: ['api-type-recognized'] }) });
  c.type('#ai-url', 'https://inconnu.example.org');
  await c.click('#ai-save');
  assert.match(c.$('#ai-test-message').textContent ?? '', /Enregistrement refusé : type d’API non reconnu/);
});

test('a command is followed until the server has applied it', async () => {
  const c = await card({ later: 2 });
  await c.click('#ai-test');
  assert.deepEqual(c.sent.length, 1);
  assert.match(c.$('#ai-test-message').textContent ?? '', /^Connexion réussie/);
});

test('a command that never settles says so, and the buttons come back', async () => {
  const c = await card({ apply: () => ({ status: 'awaiting_review' }) });
  await c.click('#ai-save');
  assert.match(c.$('#ai-test-message').textContent ?? '', /n’a pas abouti \(état : awaiting_review\)/);
  assert.equal((c.$('#ai-save') as HTMLButtonElement).disabled, false);
});

test('every part names its unit, by identifier', async () => {
  const c = await card();
  const named = (attribute: string): string[] => [...new Set([...c.host.querySelectorAll(`[${attribute}]`)].map((e) => e.getAttribute(attribute) ?? ''))].sort();
  assert.deepEqual(named('data-reads'), ['ai.getSettings']);
  assert.deepEqual(named('data-writes'), ['ai.saveSettings', 'ai.testConnection']);
  assert.equal(c.$('#ai-test').dataset.writes, 'ai.testConnection');
  assert.equal(c.$('#ai-save').dataset.writes, 'ai.saveSettings');
});

test('the form holds still until the settings are read, then fills', async () => {
  let release = (): void => undefined;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const c = await card({ gate }, timing, false);
  await settle();
  assert.ok([...c.host.querySelectorAll('button, input')].every((e) => (e as HTMLInputElement).disabled), 'nothing can be touched before the read');
  release();
  await settle();
  assert.ok([...c.host.querySelectorAll('button, input')].every((e) => !(e as HTMLInputElement).disabled));
  assert.equal((c.$('#ai-url') as HTMLInputElement).value, HOME);
});

test('the form is locked while a command runs, and free again after', async () => {
  const locked: boolean[] = [];
  const view: { host?: HTMLElement } = {};
  const waiting: Timing = { sleep: async () => { locked.push([...(view.host?.querySelectorAll('button, input') ?? [])].every((e) => (e as HTMLInputElement).disabled)); }, everyMs: 0, polls: 5 };
  const c = await card({ later: 2 }, waiting);
  view.host = c.host;
  await c.click('#ai-test');
  assert.deepEqual(locked, [true, true, true]);
  assert.ok([...c.host.querySelectorAll('button, input')].every((e) => !(e as HTMLInputElement).disabled));
});

test('a save that applied is not reported as failed when the settings cannot be read again', async () => {
  let reads = 0;
  const c = await card({ read: () => { reads += 1; if (reads > 1) throw new Error('down'); return structuredClone(fixture); } });
  c.type('#ai-key', 'sk-secret');
  await c.click('#ai-save');
  assert.match(c.$('#ai-test-message').textContent ?? '', /Paramètres enregistrés, mais ils n’ont pas pu être relus/);
  assert.equal((c.$('#ai-key') as HTMLInputElement).value, '');
});

test('a result that is the one already on record is not shown as this test’s', async () => {
  const c = await card({ apply: () => ({ status: 'applied' }) });
  await c.click('#ai-test');
  assert.match(c.$('#ai-test-message').textContent ?? '', /pas encore disponible/);
  assert.doesNotMatch(c.text(), /Connexion réussie/);
});

test('a test that no type answers takes the detected type off, even on the saved URL', async () => {
  const c = await card({
    apply: () => ({ status: 'applied' }),
    read: (() => {
      let reads = 0;
      return () => ({ ...structuredClone(fixture), lastTest: reads++ === 0 ? fixture.lastTest : { ok: false, at: '2026-10-17T10:00:00Z', provider: 'opencode', latency: null, message: "type d'API non reconnu" } });
    })(),
  });
  assert.match(c.$('#ai-detected').textContent ?? '', /Type d’API détecté/);
  await c.click('#ai-test');
  assert.equal(c.$('#ai-detected').textContent, '');
  assert.match(c.$('#ai-test-message').textContent ?? '', /reconnu/);
});

test('the saved key’s mask is suggested only for the saved URL', async () => {
  const c = await card({ read: () => ({ ...structuredClone(fixture), provider: 'cloud' }) });
  assert.match((c.$('#ai-key') as HTMLInputElement).placeholder, /sk-ant-/);
  c.type('#ai-url', 'http://192.168.1.40:4096');
  assert.equal((c.$('#ai-key') as HTMLInputElement).placeholder, 'Aucune clé');
});

test('a failed last test with no message still says why in words', async () => {
  const c = await card({ read: () => ({ ...structuredClone(fixture), lastTest: { ok: false, at: '2026-10-14T19:02:00', provider: 'opencode', latency: null, message: '' } }) });
  assert.match(c.$('#ai-test-message').textContent ?? '', /Dernier test échoué le .* : connexion impossible$/);
});
