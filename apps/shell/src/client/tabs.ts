// The tabs of a framed screen (server/frames.ts): a click on a tab brings its
// panel forward, and so does the event on the page's bus the tab names in
// `data-opens` — an element opened from the panel, the sheet or the
// suggestions brings the sheet's tab forward, whatever is at the centre.
// A tab may also name in `data-sets` (`<context>:<key>`) the context value its
// event sets from the event's `detail`, so the Micro-UI that follows it (the
// battlemap's instance) shows what was asked for. The sheet listens to the
// bus itself and reads its `detail` there.
// Every panel stays mounted; only the shown one is not `hidden`.

import type { Context } from './context.js';

export function wireTabs(root: Document, context?: Context): void {
  for (const group of root.querySelectorAll<HTMLElement>('[data-tabs]')) {
    const tabs = [...group.querySelectorAll<HTMLElement>('[role="tab"]')];
    const panels = tabs.map((tab) => root.getElementById(tab.getAttribute('aria-controls') ?? ''));
    const select = (shown: number): void => {
      tabs.forEach((tab, at) => tab.setAttribute('aria-selected', String(at === shown)));
      panels.forEach((panel, at) => {
        if (panel) panel.hidden = at !== shown;
      });
    };
    tabs.forEach((tab, at) => {
      tab.addEventListener('click', () => select(at));
      const { opens, sets } = tab.dataset;
      if (!opens) return;
      root.addEventListener(opens, (event) => {
        // The value goes into the context first, so the panel is shown already following it.
        if (sets && context) {
          const [name = '', key = ''] = sets.split(':');
          const value = ((event as CustomEvent<Record<string, unknown> | null>).detail ?? {})[key];
          if (name && typeof value === 'string' && value) context.set(name, value);
        }
        select(at);
      });
    });
  }
}
