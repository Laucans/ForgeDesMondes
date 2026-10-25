// The page of a frozen screen: the Micro-UIs its layout names, in the order
// it froze them, each as the host element the composer mounts by identifier
// (`<div data-component="<MicroUi>" data-props='{…}'>`, as the mock-up's
// pages do). The same screen gives the same page, byte for byte.

import { FRAMES, type Frame } from './frames.js';
import type { Placement, Screen } from './screens.js';

const escape = (text: string): string =>
  text.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/'/g, '&#39;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

function document(title: string, body: string, scripts: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escape(title)}</title>
<link rel="stylesheet" href="/assets/tokens.css">
<link rel="stylesheet" href="/assets/kit.css">
<link rel="stylesheet" href="/assets/shell.css">
</head>
<body>
${body}${scripts}</body>
</html>
`;
}

/** The home of the shell: the frozen screens there are. */
export function renderHome(screens: string[]): string {
  const items = screens.map((name) => `    <li><a href="/screens/${escape(encodeURIComponent(name))}">${escape(name)}</a></li>`).join('\n');
  const list = screens.length ? `  <ul class="shell-screens">\n${items}\n  </ul>\n` : '  <p class="shell-empty">No screen is frozen yet.</p>\n';
  return document('ForgeDesMondes', `<main class="shell-home">\n  <h1>ForgeDesMondes</h1>\n${list}</main>\n`, '');
}

const host = ({ microUi, props }: Placement): string =>
  `<div data-component="${escape(microUi)}" data-props="${escape(JSON.stringify(props))}"></div>`;

/** A screen with a frame: each Micro-UI where the frame puts it; the ones it does not name close the page, in the screen's order. */
function framed(screen: Screen, frame: Frame): string {
  const placed = new Map(screen.layout.map((placement) => [placement.microUi, placement]));
  const take = (microUis: string[]): Placement[] => microUis.flatMap((name) => placed.get(name) ?? []);
  const tabs = frame.centre.tabs.flatMap((tab, index) => {
    const placement = placed.get(tab.microUi);
    return placement ? [{ tab, index, placement }] : [];
  });
  const id = (kind: string, index: number): string => `${screen.screen}-centre-${kind}-${index}`;
  const buttons = tabs
    .map(({ tab, index }, at) => {
      const opens = tab.opens ? ` data-opens="${escape(tab.opens)}"` : '';
      return `<button type="button" role="tab" id="${id('tab', index)}" aria-controls="${id('panel', index)}" aria-selected="${at === 0}"${opens}>${escape(tab.label)}</button>`;
    })
    .join('');
  const panels = tabs
    .map(({ index, placement }, at) => `<div role="tabpanel" id="${id('panel', index)}" aria-labelledby="${id('tab', index)}"${at === 0 ? '' : ' hidden'}>${host(placement)}</div>`)
    .join('\n');
  const shown = new Set([...frame.header, ...frame.left.microUis, ...frame.overlay, ...tabs.map(({ tab }) => tab.microUi)]);
  const rest = screen.layout.filter(({ microUi }) => !shown.has(microUi));
  const lines = [
    `<header class="shell-frame__header">${take(frame.header).map(host).join('')}</header>`,
    `<div class="shell-frame" data-screen="${escape(screen.screen)}" data-version="${screen.version}">`,
    `<aside class="shell-frame__left" aria-label="${escape(frame.left.label)}">${take(frame.left.microUis).map(host).join('')}</aside>`,
    `<main class="shell-frame__centre">`,
    tabs.length ? `<div class="kit-tabs" data-tabs>\n<div role="tablist" aria-label="${escape(frame.centre.label)}">${buttons}</div>\n${panels}\n</div>` : '',
    `</main>`,
    `</div>`,
    ...take(frame.overlay).map(host),
    ...rest.map(host),
  ];
  return lines.filter(Boolean).join('\n') + '\n';
}

export function renderScreen(screen: Screen): string {
  const frame = FRAMES[screen.screen];
  const body = frame
    ? framed(screen, frame)
    : `<main class="shell-screen" data-screen="${escape(screen.screen)}" data-version="${screen.version}">\n${screen.layout.map((placement) => `  ${host(placement)}`).join('\n')}\n</main>\n`;
  return document(screen.screen, body, '<script type="module" src="/assets/composer.js"></script>\n');
}
