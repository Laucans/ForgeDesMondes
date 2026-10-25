// The composer: replays the screen the page froze. Every element the page
// lays out as `<div data-component="<MicroUi>" data-props='{…}'>` is filled by
// the Micro-UI of that identifier, which the shell serves at
// `/micro-ui/<MicroUi>.js` — a Micro-UI exports `mount(host, props, shell)`.
// Nothing here decides which Micro-UIs a screen has or how they are ordered:
// the page says so, as it was frozen.

import type { Props } from './context.js';
import { createDictation, type Dictation } from './dictation.js';
import { createShell, type Shell } from './shell.js';

/** What a Micro-UI's module exports; the object `mount` answers follows its props when it has `update`. */
export interface MicroUiModule {
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

/** Mounts every Micro-UI under `root`, props resolved from the screen's context and kept current. */
export async function composeScreen(root: ParentNode, shell: Shell, loader: Loader = load): Promise<void> {
  const hosts = [...root.querySelectorAll<HTMLElement>('[data-component]')];
  await Promise.all(
    hosts.map(async (host) => {
      const microUi = host.dataset.component ?? '';
      const raw = parse(host.dataset.props);
      try {
        const module = await loader(microUi);
        const mounted = module.mount(host, shell.context.resolve(raw), shell, undefined, createDictation(microUi));
        const follows = shell.context.follows(raw);
        if (follows.length && mounted && mounted.update) {
          const update = mounted.update.bind(mounted);
          shell.context.on((name) => {
            if (follows.includes(name)) update(shell.context.resolve(raw));
          });
        }
        host.dataset.state = 'mounted';
      } catch (error) {
        console.error(`shell: ${microUi} did not mount`, error);
        unavailable(host, microUi);
      }
    }),
  );
}

if (typeof document !== 'undefined') void composeScreen(document, createShell());
