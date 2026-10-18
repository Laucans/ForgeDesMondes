// The list's markup, from mock/components/SessionList.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { CREATE_SESSION, LIST_CHARACTERS, LIST_SESSIONS } from './ids.ts';
import { isPlayed, presentName, statusLabel } from './model.ts';
import type { Character, Session } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** « sam. 5 septembre », the day a session is planned or was played. */
function day(date: string): string {
  const when = new Date(`${date}T12:00:00`);
  return Number.isNaN(when.getTime()) ? date : when.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric', month: 'long' });
}

/** The list, its regions empty: the title and the button are there from the first paint. */
export function skeleton(): string {
  return (
    '<section class="panel mf-sessions" id="session-list" aria-labelledby="sessions-title">' +
    '<div class="panel__head"><h2 id="sessions-title">Séances</h2>' +
    '<div class="kit-button kit-button--bare kit-button--small">' +
    `<button type="button" class="primary" id="sessions-new" data-writes="${CREATE_SESSION}">Nouvelle séance</button>` +
    '</div></div>' +
    `<ol class="mf-sessions__list" id="sessions-items" data-reads="${LIST_SESSIONS}"></ol>` +
    '<p class="mf-sessions__status" id="sessions-status" aria-live="polite"></p>' +
    '</section>'
  );
}

function badges(session: Session): string {
  const tone = isPlayed(session) ? 'success' : 'accent';
  return (
    '<div class="kit-badge">' +
    `<span class="${tone}">${esc(statusLabel(session))}</span>` +
    (session.date === null ? '' : `<span>${esc(day(session.date))}</span>`) +
    '</div>'
  );
}

/** The kit's Button, small, one link to another page of the product each. */
function links(session: Session): string {
  const link = (label: string, href: string, variant: string): string => `<button type="button" class="${variant}" data-href="${href}">${esc(label)}</button>`;
  return (
    '<div class="kit-button kit-button--bare kit-button--small">' +
    (isPlayed(session)
      ? link('Résumé et journal', 'preparation-seance.html', 'ghost')
      : link('Préparer', 'preparation-seance.html', 'secondary') + link('Lancer en direct', 'dm-view.html', 'primary')) +
    '</div>'
  );
}

/** One session: its number, title, status and day, its summary, the PCs who were there. */
function item(session: Session, characters: readonly Character[]): string {
  const there = session.present.map((id) => presentName(characters, id)).join(', ') || '—';
  return (
    `<li class="mf-sessions__item${isPlayed(session) ? '' : ' is-next'}" id="session-${esc(session.id)}">` +
    `<span class="mf-sessions__num">${session.order}</span><div class="mf-sessions__main">` +
    `<div class="panel__head"><h3>${esc(session.title)}</h3>${badges(session)}</div>` +
    (session.summary === null ? '' : `<p class="mf-sessions__summary">${esc(session.summary)}</p>`) +
    `<p class="small muted" data-reads="${LIST_CHARACTERS}">PJ présents : ${esc(there)}</p>` +
    `${links(session)}</div></li>`
  );
}

export const items = (sessions: readonly Session[], characters: readonly Character[]): string => sessions.map((session) => item(session, characters)).join('');

/** The creation dialog: the kit's Dialog, with its Inputs and its Checkbox of the campaign's PCs. */
export function dialog(p: { title: string; plannedOn: string; pcs: readonly Character[]; present: readonly string[] }): string {
  const ticked = new Set(p.present);
  const input = (id: string, label: string, value: string, type: string): string =>
    `<div class="kit-input is-wide"><label>${label}<input type="${type}" id="${id}"${attr('value', value)}></label></div>`;
  const options = p.pcs
    .map((pc) => `<label><input type="checkbox" name="present" value="${esc(pc.id)}"${ticked.has(pc.id) ? ' checked' : ''}><span>${esc(pc.name)}</span></label>`)
    .join('');
  return (
    '<div class="mf-sessions__modal" id="session-create-dialog"><div class="kit-dialog"><div class="kit-dialog__veil">' +
    '<div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="Nouvelle séance">' +
    '<header class="kit-dialog__header"><h2 class="kit-dialog__title">Nouvelle séance</h2>' +
    '<button class="kit-dialog__close" type="button" aria-label="Fermer" id="session-create-close">×</button></header>' +
    '<div class="kit-dialog__body">' +
    input('session-create-title', 'Titre', p.title, 'text') +
    input('session-create-date', 'Date', p.plannedOn, 'date') +
    `<div class="kit-checkbox"><fieldset id="session-create-present" data-reads="${LIST_CHARACTERS}"><legend>PJ présents</legend>${options}</fieldset></div>` +
    '<p class="small muted">Vous pourrez retirer ou ajouter des PJ dans la préparation.</p>' +
    '<p class="mf-sessions__error" id="session-create-error" aria-live="polite"></p>' +
    '</div><footer class="kit-dialog__footer">' +
    '<button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="session-create-cancel">Annuler</button>' +
    `<button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="session-create-submit" data-writes="${CREATE_SESSION}">Créer la séance</button>` +
    '</footer></div></div></div></div>'
  );
}
