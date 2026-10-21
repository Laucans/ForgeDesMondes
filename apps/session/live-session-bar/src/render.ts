// The bar's markup, from mock/components/LiveSessionBar.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part that
// shows what a Capability answers names it (`data-reads`), every actuator the
// DataCapability it triggers (`data-writes`), by identifier.

import { CLOSE_SESSION, CLOSE_WINDOW, GET_DIFFUSION, GET_LIVE_SESSION, GET_PREVIOUS_SESSION_SUMMARY, GET_SESSION_JOURNAL, SHOW_COVER } from './ids.ts';
import { linesLabel, showing } from './model.ts';
import type { Diffusion, Live, Recap } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const button = (id: string, label: string, variant: string, extra = ''): string => `<button type="button" class="${variant}" id="${id}"${extra}>${esc(label)}</button>`;

/** The bar, its regions empty: the title and the actions are there from the first paint. */
export function skeleton(): string {
  const live = `data-reads="${GET_LIVE_SESSION}"`;
  return (
    `<section class="mf-live-bar" id="session-bar" aria-labelledby="session-bar-title" ${live}>` +
    '<div class="mf-live-bar__title"><h1 id="session-bar-title"></h1>' +
    `<span class="small muted" id="session-bar-meta" ${live}></span></div>` +
    `<div class="kit-badge" id="session-bar-badges" ${live}></div>` +
    `<nav class="mf-live-bar__scenes" id="session-bar-scenes" aria-label="Scènes de la séance" ${live}></nav>` +
    `<div class="mf-live-bar__broadcast" id="session-bar-broadcast" data-reads="${GET_DIFFUSION}">` +
    '<span class="small" id="session-bar-showing" aria-live="polite"></span><div class="kit-button kit-button--bare kit-button--small" id="session-bar-broadcast-actions"></div></div>' +
    '<div class="kit-button kit-button--bare kit-button--small" id="session-bar-actions">' +
    button('session-bar-player', 'Ouvrir la player view', 'secondary', ' data-href="player-view.html" data-target="_blank"') +
    button('session-bar-pilot', 'Player view pilotée', 'ghost', ' data-href="player-view.html?pilote=1" data-target="_blank"') +
    button('session-bar-close', 'Clore la séance', 'danger', ` data-writes="${CLOSE_SESSION}"`) +
    '</div>' +
    '<div class="mf-modal" id="session-close-modal" hidden></div></section>'
  );
}

export const heading = (live: Live): string => `Séance ${live.order} — ${live.title}`;

export const meta = (live: Live): string => (live.startedAt === '' ? 'En direct' : `En direct depuis ${live.startedAt}`);

export function badges(live: Live, closed: boolean): string {
  const mic = live.micOn === null ? '' : live.micOn && !closed ? '<span class="accent">Micro actif</span>' : '<span>Micro en pause</span>';
  return (closed ? '<span class="danger">Close</span>' : '<span class="success">En direct</span>') + mic;
}

export const scenes = (live: Live): string =>
  live.scenes
    .map((s, i) => `<button type="button" class="chip${i === live.currentScene ? ' is-on' : ''}" data-scene="${i}" id="session-bar-scene-${i + 1}" title="${esc(s.title)}" aria-pressed="${i === live.currentScene}">Scène ${s.order} · ${esc(s.title)}</button>`)
    .join('');

export function showingHtml(diffusion: Diffusion): string {
  const what = showing(diffusion);
  return `Les joueurs voient <b>${esc(what.base)}</b>` + (what.window === null ? '' : ` + <b>${esc(what.window)}</b>`);
}

/** The buttons that undo what is shown: back to the cover, close the floating window; only the ones that apply. */
export function broadcastActions(diffusion: Diffusion): string {
  return (
    (diffusion.base.kind === 'cover' ? '' : button('session-bar-cover', 'Revenir à la couverture', 'ghost', ` data-writes="${SHOW_COVER}"`)) +
    (diffusion.window === null ? '' : button('session-bar-close-window', 'Fermer la fenêtre', 'ghost', ` data-writes="${CLOSE_WINDOW}"`))
  );
}

/** The kit's Dialog: a title, a body, a footer of buttons. */
function dialog(title: string, body: string, footer: string): string {
  return (
    '<div class="kit-dialog"><div class="kit-dialog__veil">' +
    `<div class="kit-dialog__window" role="dialog" aria-modal="true" aria-labelledby="session-close-title"><header class="kit-dialog__header"><h2 class="kit-dialog__title" id="session-close-title">${esc(title)}</h2>` +
    '<button class="kit-dialog__close" type="button" id="session-close-x" aria-label="Fermer">×</button></header>' +
    `<div class="kit-dialog__body">${body}</div><footer class="kit-dialog__footer">${footer}</footer></div></div></div>`
  );
}

const dialogButton = (id: string, label: string, variant: 'primary' | 'ghost', extra = ''): string =>
  `<button type="button" class="kit-dialog__btn kit-dialog__btn--${variant}" id="${id}"${extra}>${esc(label)}</button>`;

/** Before the close: what it does, and a way back. `problem` is why the last try failed. */
export function confirmDialog(live: Live, problem = '', busy = false): string {
  return dialog(
    `Clore la séance ${live.order}`,
    '<p class="kit-dialog__text">Le micro sera mis en pause et les sons arrêtés. Le journal transcrit est conservé sur cette machine ; un résumé est généré avec le modèle configuré.</p>' +
      '<p class="small muted">Une séance close ne se rouvre pas depuis la barre.</p>' +
      `<p class="mf-live-bar__status${problem === '' ? '' : ' mf-live-bar__error'}" id="session-close-status" role="status" aria-live="polite">${esc(busy ? 'Clôture en cours…' : problem)}</p>`,
    dialogButton('session-close-cancel', 'Continuer la séance', 'ghost', busy ? ' disabled' : '') +
      dialogButton('session-close-submit', 'Clore la séance', 'primary', ` data-writes="${CLOSE_SESSION}"${busy ? ' disabled' : ''}`),
  );
}

/** After the close: the journal kept, the time played, the model and the summary. */
export function recapDialog(live: Live, recap: Recap, problem = ''): string {
  const row = (id: string, label: string, value: string, reads: string): string => `<li class="kit-dialog__row" id="${id}" data-reads="${reads}"><span>${esc(label)}</span><strong>${esc(value)}</strong></li>`;
  const summary = `data-reads="${GET_PREVIOUS_SESSION_SUMMARY}"`;
  return dialog(
    `Séance ${live.order} close`,
    '<p class="kit-dialog__text">Le micro est en pause et les sons sont arrêtés. Le journal transcrit est conservé sur cette machine.</p>' +
      (problem === '' ? '' : `<p class="mf-live-bar__error" id="session-close-status" role="status">${esc(problem)}</p>`) +
      '<ul class="kit-dialog__list">' +
      row('session-close-lines', 'Lignes de journal', linesLabel(recap.journalLines), GET_SESSION_JOURNAL) +
      (recap.duration === '' ? '' : row('session-close-duration', 'Durée', recap.duration, GET_LIVE_SESSION)) +
      (recap.summaryModel === '' ? '' : row('session-close-model', 'Modèle du résumé', recap.summaryModel, GET_PREVIOUS_SESSION_SUMMARY)) +
      '</ul>' +
      '<div class="mf-live-bar__summary"><h3>Résumé de la séance</h3>' +
      `<p id="session-close-summary" ${summary}>${esc(recap.summary === '' ? 'Aucun résumé n’a pu être généré.' : recap.summary)}</p>` +
      (recap.mentions.length === 0 ? '' : `<h3>Entités mentionnées</h3><div class="row" id="session-close-mentions" ${summary}>${recap.mentions.map((m) => `<span class="chip">${esc(m)}</span>`).join('')}</div>`) +
      (recap.threads.length === 0 ? '' : `<h3>Fils ouverts</h3><ul class="list" ${summary}>${recap.threads.map((t, i) => `<li class="item" id="session-close-thread-${i + 1}">${esc(t)}</li>`).join('')}</ul>`) +
      '</div><p class="small muted">Ce résumé apparaîtra en tête de la préparation de la séance suivante.</p>',
    dialogButton('session-close-done', 'Retour à la campagne', 'primary', ' data-href="campagne.html"'),
  );
}
