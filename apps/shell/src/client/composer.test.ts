import assert from 'node:assert/strict';
import { test } from 'node:test';
import { composeScreen, type MicroUiModule } from './composer.js';
import { Context } from './context.js';
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

type FakeHost = { dataset: Record<string, string>; isConnected?: boolean };

/** A page whose markup changes: `draw` adds a host, and the watch the composer was given tells it so, as a MutationObserver would. */
function page(...microUis: string[]) {
  const hosts: FakeHost[] = microUis.map((microUi) => ({ dataset: { component: microUi, props: '{}' } }));
  let changed = () => {};
  const node = { querySelectorAll: () => [...hosts] } as unknown as ParentNode;
  const watch = (_root: ParentNode, onChange: () => void) => void (changed = onChange);
  const draw = (microUi: string, props: object): FakeHost => {
    const host = { dataset: { component: microUi, props: JSON.stringify(props) } };
    hosts.push(host);
    return host;
  };
  const settle = () => new Promise((resolve) => setImmediate(resolve));
  return { node, watch, draw, change: () => changed(), settle };
}

// AC1: a host a mounted Micro-UI adds to its markup is mounted with its props, its data-state 'mounted', once
test('a host a mounted Micro-UI draws in its own markup is mounted once, with its props', async () => {
  const screen = page('PlayerStage');
  const mounted: Array<{ microUi: string; host: unknown; props: unknown }> = [];
  const loader = async (microUi: string): Promise<MicroUiModule> => ({
    mount: (host, props) => {
      mounted.push({ microUi, host, props });
      if (microUi === 'PlayerStage') nested = screen.draw('Battlemap', { instance: 'bm-1', view: 'player' });
    },
  });
  let nested: FakeHost | undefined;
  await composeScreen(screen.node, shell, loader, screen.watch);
  screen.change();
  await screen.settle();
  screen.change();
  await screen.settle();
  assert.deepEqual(
    mounted.map((m) => m.microUi),
    ['PlayerStage', 'Battlemap'],
  );
  assert.equal(mounted[1]?.host, nested);
  assert.deepEqual(mounted[1]?.props, { instance: 'bm-1', view: 'player' });
  assert.equal(nested?.dataset.state, 'mounted');
});

// AC2: a host already mounted is not mounted a second time when the markup changes elsewhere
test('a host already mounted is not mounted again when the markup changes', async () => {
  const screen = page('PlayerStage', 'MediaLibrary');
  const mounts: string[] = [];
  // The loader answers late: the change comes while the first mounts are still loading.
  const loader = async (microUi: string): Promise<MicroUiModule> => {
    await screen.settle();
    return { mount: () => void mounts.push(microUi) };
  };
  const composed = composeScreen(screen.node, shell, loader, screen.watch);
  screen.change();
  await composed;
  screen.draw('Battlemap', {});
  screen.change();
  screen.change();
  await screen.settle();
  assert.deepEqual(mounts.sort(), ['Battlemap', 'MediaLibrary', 'PlayerStage']);
});

// AC3: a nested host whose props name `$ctx.<name>` gets the resolved props in its update when the context sets the name
test('a nested host follows the context: its update receives the resolved props', async () => {
  const screen = page('PlayerStage');
  const context = new Context();
  const real = { context } as unknown as Shell;
  const updates: unknown[] = [];
  const loader = async (microUi: string): Promise<MicroUiModule> => ({
    mount: (_host, props) => (microUi === 'Battlemap' ? { update: (next) => void updates.push({ opened: props, next }) } : undefined),
  });
  await composeScreen(screen.node, real, loader, screen.watch);
  screen.draw('Battlemap', { instance: '$ctx.battlemap', view: 'player' });
  screen.change();
  await screen.settle();
  context.set('battlemap', 'bm-1');
  assert.deepEqual(updates, [{ opened: { instance: undefined, view: 'player' }, next: { instance: 'bm-1', view: 'player' } }]);
});

test('a host its parent drew over stops following the context', async () => {
  const screen = page('Battlemap');
  const context = new Context();
  const updates: unknown[] = [];
  const loader = async (): Promise<MicroUiModule> => ({ mount: () => ({ update: (props) => void updates.push(props) }) });
  const host = screen.draw('Battlemap', { instance: '$ctx.battlemap' });
  await composeScreen(screen.node, { context } as unknown as Shell, loader, screen.watch);
  context.set('battlemap', 'bm-1');
  assert.equal(updates.length, 1);
  host.isConnected = false;
  context.set('battlemap', 'bm-2');
  assert.equal(updates.length, 1);
  host.isConnected = true;
  context.set('battlemap', 'bm-3');
  assert.equal(updates.length, 1, 'once stopped, it stays stopped');
});
