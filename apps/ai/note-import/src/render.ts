// The Micro-UI's markup, from mock/components/NoteImport.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part
// names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { CREATE_ENTITIES_FROM_NOTE, EXTRACT_FROM_NOTES, LIST_IMPORTED_NOTES } from './ids.ts';
import { createLabel } from './model.ts';
import type { Import, Outcome, Proposal } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const MIC =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
const PAUSE =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';

/** The kit's Button: one actuator, naming the DataCapability it triggers when it triggers one. */
function button(p: { id: string; label: string; variant: string; writes?: string; disabled?: boolean }): string {
  const writes = p.writes === undefined ? '' : ` data-writes="${p.writes}"`;
  return `<button type="button" class="${p.variant}" id="${p.id}"${writes}${p.disabled === true ? ' disabled' : ''}>${esc(p.label)}</button>`;
}

/** The screen, its regions empty: what never depends on the notes — the title, the text area, the buttons — is there from the first paint. */
export function skeleton(): string {
  return (
    '<section class="panel mf-notes" id="note-import" aria-labelledby="notes-title">' +
    '<div class="panel__head"><h2 id="notes-title">Importer des notes</h2><span class="small muted">La note brute est toujours conservée</span></div>' +
    '<p class="muted small">Tapez, collez ou dictez vos notes en texte libre. L’IA en extrait les personnages, lieux et événements ; vous validez la liste en lot avant toute création.</p>' +
    '<div id="notes-input"><div class="kit-textarea is-wide">' +
    '<label class="kit-textarea__label" for="notes-text">Notes en texte libre</label>' +
    '<textarea class="kit-textarea__field" id="notes-text" rows="7"></textarea>' +
    '<div class="kit-textarea__footer"><p class="kit-textarea__hint">Collez vos notes existantes, ou dictez-les avec le micro.</p></div></div></div>' +
    '<div class="row" id="notes-controls">' +
    '<div class="kit-micbutton kit-micbutton--compact" id="notes-mic-box" data-state="idle">' +
    `<button type="button" class="kit-micbutton__btn" id="notes-mic" aria-pressed="false" aria-label="Dicter"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button>` +
    '<div class="kit-micbutton__text"><span class="kit-micbutton__label">Dicter</span>' +
    '<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div>' +
    `<div class="kit-button kit-button--bare">${button({ id: 'notes-extract', label: 'Extraire les éléments', variant: 'primary', writes: EXTRACT_FROM_NOTES })}</div>` +
    '</div>' +
    '<div class="mf-notes__status" id="notes-status" aria-live="polite"></div>' +
    `<div id="notes-proposals" class="stack" data-reads="${LIST_IMPORTED_NOTES}" hidden></div>` +
    '<section id="notes-history" class="stack" aria-labelledby="notes-history-title"><h3 id="notes-history-title">Notes déjà importées</h3>' +
    `<ul class="list" id="notes-history-list" data-reads="${LIST_IMPORTED_NOTES}"></ul></section>` +
    '</section>'
  );
}

/** The mic's states, in words. */
export const MIC_TEXTS = { idle: 'Prêt à dicter', recording: 'Enregistrement', paused: 'En pause' } as const;
export type MicState = keyof typeof MIC_TEXTS;

/** The checked proposals and the buttons that validate or reject them: the kit's Checkbox, then the kit's Button. */
export function proposals(list: readonly Proposal[]): string {
  const option = (p: Proposal): string =>
    `<label><input type="checkbox" name="proposal" value="${esc(p.id)}"${p.checked ? ' checked' : ''}>` +
    `<span><span class="mf-notes__type">${esc(p.type)}</span>${esc(p.name)}` +
    `${p.existing === null ? '' : ' <span class="mf-notes__dup">· existe déjà, sera complété</span>'}<small>« ${esc(p.excerpt)} »</small></span></label>`;
  const checked = list.filter((p) => p.checked).length;
  return (
    `<div class="panel__head"><h3 id="notes-proposals-title">${list.length} proposition${list.length > 1 ? 's' : ''} extraite${list.length > 1 ? 's' : ''}</h3><span class="small muted">Les doublons probables sont décochés</span></div>` +
    `<div class="kit-checkbox"><fieldset id="notes-proposals-list">${list.map(option).join('')}</fieldset></div>` +
    '<div class="kit-button kit-button--bare">' +
    button({ id: 'notes-create', label: createLabel(checked), variant: 'primary', writes: CREATE_ENTITIES_FROM_NOTE, disabled: checked === 0 }) +
    button({ id: 'notes-cancel', label: 'Tout rejeter', variant: 'ghost' }) +
    '</div>'
  );
}

const day = (date: string): string => {
  const parsed = new Date(`${date}T12:00:00`);
  return Number.isNaN(parsed.getTime()) ? date : parsed.toLocaleDateString('fr-FR', { day: 'numeric', month: 'short' });
};

/** The notes already imported, newest first as the Capability answers them. */
export const history = (imports: readonly Import[]): string =>
  imports
    .map((i) => `<li class="item" id="notes-import-${esc(i.id)}"><span>${esc(i.title)}</span><span class="small muted">${esc(day(i.date))} · ${i.chars} caractères · ${i.created}/${i.proposed} créés</span></li>`)
    .join('');

/** What the status line shows. */
export function status(outcome: Outcome): string {
  switch (outcome.kind) {
    case 'none':
      return '';
    case 'busy':
      return `<p class="item muted">${esc(outcome.text)}</p>`;
    case 'ok':
      return `<p class="item is-ok" id="notes-done">${esc(outcome.text)}</p>`;
    case 'error':
      return `<p class="item is-error" id="notes-error">${esc(outcome.text)}</p>`;
  }
}
