// The composer: replays the screen the page froze. Every element the page
// lays out as `<div data-component="<MicroUi>" data-props='{…}'>` is filled by
// the Micro-UI of that identifier, which the shell serves at
// `/micro-ui/<MicroUi>.js` — a Micro-UI exports `mount(host, props, shell)`.
// Nothing here decides which Micro-UIs a screen has or how they are ordered:
// the page says so, as it was frozen.

import type { Props } from './context.js';
import { createDictation, type Dictation } from './dictation.js';
import { createShell, type Shell } from './shell.js';
import { wireTabs } from './tabs.js';

/** What a Micro-UI's module exports; the object `mount` answers follows its props when it has `update`. */
export interface MicroUiModule {
  /** True for a Micro-UI whose fifth `mount` argument is the DM's microphone; the others get nothing there. */
  listens?: boolean;
  /** The fourth argument is the Micro-UI's timing, left to its default; the fifth is the DM's microphone, for the ones that listen. */
  mount(host: HTMLElement, props: Props, shell: Shell, timing?: undefined, dictation?: Dictation): { update?(props: Props): void } | void;
}

export type Loader = (microUi: string) => Promise<MicroUiModule>;

const load: Loader = (microUi) => import(/* @vite-ignore */ `/micro-ui/${encodeURIComponent(microUi)}.js`) as Promise<MicroUiModule>;

function parse(text: string | undefined): Props {
  try {
    const props: unknown = JSON.parse(text ?? '{}');
    return props && typeof props === 'object' && !Array.isArray(props) ? (props as Props) : {};
  } catch {
    return {};
  }
}

/** A Micro-UI the shell cannot mount says so in its place; the others carry on. */
function unavailable(host: HTMLElement, microUi: string): void {
  host.dataset.state = 'unavailable';
  const notice = host.ownerDocument.createElement('p');
  notice.className = 'shell-unavailable';
  notice.setAttribute('role', 'alert');
  notice.textContent = `${microUi} is not available`;
  host.replaceChildren(notice);
}

/** Tells `changed` each time the markup under `root` changes: a Micro-UI that draws another one's host in its own markup. */
export type Watch = (root: ParentNode, changed: () => void) => void;

const observe: Watch = (root, changed) => {
  if (typeof MutationObserver === 'undefined') return;
  new MutationObserver(() => changed()).observe(root, { childList: true, subtree: true });
};

async function mountHost(host: HTMLElement, shell: Shell, loader: Loader): Promise<void> {
  const microUi = host.dataset.component ?? '';
  const raw = parse(host.dataset.props);
  try {
    const module = await loader(microUi);
    const props = shell.context.resolve(raw);
    const mounted = module.listens === true ? module.mount(host, props, shell, undefined, createDictation(microUi)) : module.mount(host, props, shell);
    const follows = shell.context.follows(raw);
    if (follows.length && mounted && mounted.update) {
      const update = mounted.update.bind(mounted);
      const stop = shell.context.on((name) => {
        // A host its parent drew over is gone: it stops following.
        if (host.isConnected === false) stop();
        else if (follows.includes(name)) update(shell.context.resolve(raw));
      });
    }
    host.dataset.state = 'mounted';
  } catch (error) {
    console.error(`shell: ${microUi} did not mount`, error);
    unavailable(host, microUi);
  }
}

/**
 * Mounts every Micro-UI under `root`, props resolved from the screen's context and kept current. A host that
 * appears later (a mounted Micro-UI drew it in its own markup) is mounted the same way; a host is mounted once.
 */
export async function composeScreen(root: ParentNode, shell: Shell, loader: Loader = load, watch: Watch = observe): Promise<void> {
  // Marked when first seen, before the loader answers: a change of the markup meanwhile does not mount it twice.
  const seen = new WeakSet<HTMLElement>();
  const compose = (): Promise<void[]> => {
    const fresh = [...root.querySelectorAll<HTMLElement>('[data-component]')].filter((host) => !seen.has(host));
    for (const host of fresh) seen.add(host);
    return Promise.all(fresh.map((host) => mountHost(host, shell, loader)));
  };
  watch(root, () => void compose());
  await compose();
}

if (typeof document !== 'undefined') {
  const shell = createShell();
  wireTabs(document, shell.context);
  void composeScreen(document, shell);
}
