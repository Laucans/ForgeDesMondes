// The list's markup, from mock/components/SessionList.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { CREATE_SESSION, LIST_CHARACTERS, LIST_SESSIONS } from './ids.ts';
import { dateLabel, firstName, isNext, playerCharacters, statusLabel, statusTone } from './model.ts';
import type { Character, Form, Notice, Reading, Session } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** The list, its regions empty: the title and the button that opens the form are there from the first paint, the button held until the sessions are read. */
export function skeleton(): string {
  return (
    '<section class="panel mf-sessions" id="session-list" aria-labelledby="sessions-title">' +
    '<div class="panel__head"><h2 id="sessions-title">Séances</h2>' +
    '<div class="kit-button kit-button--bare kit-button--small">' +
    `<button type="button" class="primary" id="sessions-new" data-writes="${CREATE_SESSION}" disabled>Nouvelle séance</button></div></div>` +
    `<ol class="mf-sessions__list" id="sessions-items" data-reads="${LIST_SESSIONS}" aria-live="polite"></ol>` +
    '<div id="sessions-dialog"></div>' +
    '</section>'
  );
}

function kitButtons(buttons: ReadonlyArray<{ label: string; variant: string; href: string }>): string {
  return (
    '<div class="kit-button kit-button--bare kit-button--small">' +
    buttons.map((b) => `<button type="button" class="${b.variant}"${attr('data-href', b.href)}>${esc(b.label)}</button>`).join('') +
    '</div>'
  );
}

function item(session: Session, characters: readonly Character[]): string {
  const next = isNext(session);
  const date = dateLabel(session.date);
  const names = session.present.map((id) => firstName(characters, id)).join(', ') || '—';
  return (
    `<li class="mf-sessions__item${next ? ' is-next' : ''}" id="session-${esc(session.id)}"><span class="mf-sessions__num">${session.order}</span><div class="mf-sessions__main">` +
    `<div class="panel__head"><h3>${esc(session.title)}</h3><div class="kit-badge"><span class="${statusTone(session)}">${esc(statusLabel(session))}</span>${date === '' ? '' : `<span>${esc(date)}</span>`}</div></div>` +
    (session.summary ? `<p class="mf-sessions__summary">${esc(session.summary)}</p>` : '') +
    `<p class="small muted" data-reads="${LIST_CHARACTERS}">PJ présents : ${esc(names)}</p>` +
    kitButtons(
      session.status === 'live'
        ? [{ label: 'Reprendre en direct', href: 'dm-view.html', variant: 'primary' }]
        : next
          ? [
              { label: 'Préparer', href: 'preparation-seance.html', variant: 'secondary' },
              { label: 'Lancer en direct', href: 'dm-view.html', variant: 'primary' },
            ]
          : [{ label: 'Résumé et journal', href: 'preparation-seance.html', variant: 'ghost' }],
    ) +
    '</div></li>'
  );
}

/** The sessions in the order given, or what to say instead: while they load, or when they cannot be read. */
export function items(sessions: readonly Session[], characters: readonly Character[], reading: Reading): string {
  if (reading === 'loading') return '<li class="mf-sessions__empty muted">Chargement des séances…</li>';
  if (reading === 'failed') return '<li class="mf-sessions__empty is-error">Impossible de lire les séances.</li>';
  if (sessions.length === 0) return '<li class="mf-sessions__empty muted">Aucune séance pour l’instant.</li>';
  return sessions.map((session) => item(session, characters)).join('');
}

/** The kit's Input, a label over a field. */
function input(p: { id: string; label: string; type: string; value: string; wide: boolean }): string {
  return `<div class="kit-input${p.wide ? ' is-wide' : ''}"><label>${esc(p.label)}<input type="${p.type}" id="${p.id}"${attr('value', p.value)}></label></div>`;
}

/** The kit's Checkbox: the PCs only (present-only-pj), each ticked as the form holds it. */
function presentChoice(characters: readonly Character[], form: Form): string {
  const options = playerCharacters(characters)
    .map((c) => `<label><input type="checkbox" name="present" value="${esc(c.id)}"${form.present.includes(c.id) ? ' checked' : ''}><span>${esc(c.name)}</span></label>`)
    .join('');
  return `<div class="kit-checkbox"><fieldset id="session-create-present" data-reads="${LIST_CHARACTERS}"><legend>PJ présents</legend>${options}</fieldset></div>`;
}

/** What the form says of the command it sent. */
export function notice(state: Notice): string {
  switch (state.kind) {
    case 'none':
      return '';
    case 'busy':
      return '<p class="muted">Création en cours…</p>';
    case 'error':
      return `<p class="is-error" id="session-create-message">${esc(state.text)}</p>`;
  }
}

/** The form to name the next session, as the kit's Dialog opened over the page. */
export function dialog(form: Form, characters: readonly Character[]): string {
  return (
    '<div class="mf-modal" id="session-create-dialog"><div class="kit-dialog"><div class="kit-dialog__veil">' +
    '<div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="Nouvelle séance">' +
    '<header class="kit-dialog__header"><h2 class="kit-dialog__title">Nouvelle séance</h2>' +
    '<button class="kit-dialog__close" type="button" aria-label="Fermer">×</button></header>' +
    '<div class="kit-dialog__body">' +
    input({ id: 'session-create-title', label: 'Titre', type: 'text', value: form.title, wide: true }) +
    input({ id: 'session-create-date', label: 'Date', type: 'date', value: form.date, wide: false }) +
    presentChoice(characters, form) +
    '<p class="small muted">Vous pourrez retirer ou ajouter des PJ dans la préparation.</p>' +
    '<div id="session-create-notice" aria-live="polite"></div>' +
    '</div>' +
    '<footer class="kit-dialog__footer">' +
    '<button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="session-create-cancel">Annuler</button>' +
    `<button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="session-create-submit" data-writes="${CREATE_SESSION}">Créer la séance</button>` +
    '</footer></div></div></div></div>'
  );
}
