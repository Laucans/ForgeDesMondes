// The card's markup, from mock/components/PreviousSummary.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part that
// shows what a Capability answers names it (`data-reads`) by identifier.

import { GET_PREVIOUS_SESSION_SUMMARY, GET_SESSION_JOURNAL } from './ids.ts';
import { chipKind } from './model.ts';
import type { Journal, Summary } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** The card, its regions empty: the title, the button and the closed panel are there from the first paint. */
export function skeleton(): string {
  const summary = `data-reads="${GET_PREVIOUS_SESSION_SUMMARY}"`;
  return (
    '<section class="panel mf-previous" id="previous-summary" aria-labelledby="previous-title">' +
    '<div class="panel__head"><h2 id="previous-title">Séance précédente</h2>' +
    '<div class="kit-button kit-button--bare kit-button--small">' +
    '<button type="button" class="secondary" id="previous-journal-open">Journal complet</button></div></div>' +
    `<p id="previous-summary-text" class="mf-previous__text" ${summary} aria-live="polite"></p>` +
    `<p class="small muted" id="previous-meta" ${summary}></p>` +
    '<div class="layout-2 layout-2--even">' +
    '<section id="previous-mentions" aria-labelledby="previous-mentions-title" class="stack">' +
    `<h3 id="previous-mentions-title">Entités mentionnées</h3><div class="row" id="previous-mentions-list" ${summary}></div></section>` +
    '<section id="previous-threads" aria-labelledby="previous-threads-title" class="stack">' +
    `<h3 id="previous-threads-title">Fils ouverts</h3><ul class="list" id="previous-threads-list" ${summary}></ul></section>` +
    '</div>' +
    '<aside class="mf-previous__journal" id="previous-journal" aria-labelledby="previous-journal-title" hidden>' +
    '<div class="panel__head"><h2 id="previous-journal-title">Journal transcrit</h2>' +
    '<button type="button" class="icon-btn" id="previous-journal-close" aria-label="Fermer le journal">×</button></div>' +
    '<p class="small muted" id="previous-journal-meta" aria-live="polite"></p>' +
    `<ol class="mf-previous__lines" id="previous-journal-lines" data-reads="${GET_SESSION_JOURNAL}"></ol>` +
    '</aside></section>'
  );
}

export const mentions = (summary: Summary): string =>
  summary.mentions
    .map((m) => `<a class="chip chip--${chipKind(m.type)}" id="previous-mention-${esc(m.id)}" href="fiche-entite.html?id=${encodeURIComponent(m.id)}">${esc(m.name)}</a>`)
    .join('');

export const threads = (summary: Summary): string => summary.threads.map((t, i) => `<li id="previous-thread-${i + 1}">${esc(t)}</li>`).join('');

export const lines = (journal: Journal): string => journal.journal.map((l) => `<li><time>${esc(l.t)}</time><span>${esc(l.text)}</span></li>`).join('');
