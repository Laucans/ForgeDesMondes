// The page of a frozen screen: the Micro-UIs its layout names, in the order
// it froze them, each as the host element the composer mounts by identifier
// (`<div data-component="<MicroUi>" data-props='{…}'>`, as the mock-up's
// pages do). The same screen gives the same page, byte for byte.

import type { Screen } from './screens.js';

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
  const items = screens.map((name) => `    <li><a href="/screens/${escape(name)}">${escape(name)}</a></li>`).join('\n');
  const list = screens.length ? `  <ul class="shell-screens">\n${items}\n  </ul>\n` : '  <p class="shell-empty">No screen is frozen yet.</p>\n';
  return document('ForgeDesMondes', `<main class="shell-home">\n  <h1>ForgeDesMondes</h1>\n${list}</main>\n`, '');
}

export function renderScreen(screen: Screen): string {
  const placed = screen.layout
    .map(({ microUi, props }) => `  <div data-component="${escape(microUi)}" data-props="${escape(JSON.stringify(props))}"></div>`)
    .join('\n');
  const main = `<main class="shell-screen" data-screen="${escape(screen.screen)}" data-version="${screen.version}">\n${placed}\n</main>\n`;
  return document(screen.screen, main, '<script type="module" src="/assets/composer.js"></script>\n');
}
