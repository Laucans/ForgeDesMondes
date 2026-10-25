// The tabs of a framed screen (server/frames.ts): a click on a tab brings its
// panel forward, and so does the event on the page's bus the tab names in
// `data-opens` — an element opened from the panel, the sheet or the
// suggestions brings the sheet's tab forward, whatever is at the centre.
// Every panel stays mounted; only the shown one is not `hidden`.

export function wireTabs(root: Document): void {
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
      if (tab.dataset.opens) root.addEventListener(tab.dataset.opens, () => select(at));
    });
  }
}
