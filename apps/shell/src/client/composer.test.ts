import assert from 'node:assert/strict';
import { test } from 'node:test';
import { composeScreen, type MicroUiModule } from './composer.js';
import type { Shell } from './shell.js';

/** A root holding one host per Micro-UI, enough for the composer: it reads `dataset` and nothing else of them. */
function root(...microUis: string[]) {
  const hosts = microUis.map((microUi) => ({ dataset: { component: microUi, props: '{}' } }));
  return { hosts, node: { querySelectorAll: () => hosts } as unknown as ParentNode };
}

const shell = { context: { resolve: (props: object) => props, follows: () => [], on: () => undefined } } as unknown as Shell;

test('a Micro-UI is mounted with the shell, its timing left to it, and a dictation of its own id', async () => {
  const mounts: Array<{ args: unknown[] }> = [];
  const loader = async (): Promise<MicroUiModule> => ({
    listens: true,
    mount: (...args: unknown[]) => void mounts.push({ args }),
  });
  const { node, hosts } = root('OralBrief');
  await composeScreen(node, shell, loader);
  assert.equal(mounts.length, 1);
  const [host, props, given, timing, dictation] = mounts[0]?.args ?? [];
  assert.equal(host, hosts[0]);
  assert.deepEqual(props, {});
  assert.equal(given, shell);
  assert.equal(timing, undefined);
  const heard = dictation as { transcript(): string; start(): void; audio(): Promise<string> };
  assert.equal(heard.transcript(), '');
  // The test runs where no browser has a microphone: the take is the id of the Micro-UI that mounts.
  heard.start();
  assert.equal(Buffer.from(await heard.audio(), 'base64').toString(), 'OralBrief');
});

test('each Micro-UI is handed a dictation that names it', async () => {
  const takes: Array<Promise<string>> = [];
  const loader = async (): Promise<MicroUiModule> => ({
    listens: true,
    mount: (_host, _props, _shell, _timing, dictation) => {
      assert.ok(dictation);
      dictation.start?.();
      takes.push(Promise.resolve(dictation.audio()));
    },
  });
  await composeScreen(root('OralBrief', 'NoteImport').node, shell, loader);
  const heard = (await Promise.all(takes)).map((take) => Buffer.from(take, 'base64').toString());
  assert.deepEqual(heard.sort(), ['NoteImport', 'OralBrief']);
});

test('a Micro-UI that does not listen is mounted with the three arguments it knows, nothing in the slots after', async () => {
  const mounts: unknown[][] = [];
  const loader = async (): Promise<MicroUiModule> => ({ mount: (...args: unknown[]) => void mounts.push(args) });
  await composeScreen(root('PlayerStage', 'MediaLibrary', 'LiveSoundbox').node, shell, loader);
  assert.equal(mounts.length, 3);
  for (const args of mounts) assert.equal(args.length, 3);
});
