// The card's markup, from mock/components/SoundLibrary.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { ADD_SOUND, GET_LIVE_SESSION, LIST_SOUNDS, REMOVE_SOUND, UPDATE_SOUND } from './ids.ts';
import { FILE_TYPES, duration, norm } from './model.ts';
import type { EditForm, Outcome, Sound } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** The kit's Input (mock/kit/Input.html), wide. */
function input(p: { id: string; label: string; value?: string; hint?: string; type?: string; placeholder?: string }): string {
  return (
    `<div class="kit-input is-wide"><label>${esc(p.label)}` +
    `<input type="${p.type ?? 'text'}" id="${p.id}"${attr('value', p.value ?? '')}${attr('placeholder', p.placeholder ?? '')}>` +
    (p.hint ? `<small>${esc(p.hint)}</small>` : '') +
    '</label></div>'
  );
}

/** The kit's Button (mock/kit/Button.html), bare: each one tied to the DataCapability it triggers. */
function buttons(items: ReadonlyArray<{ id: string; label: string; variant: string; writes?: string }>): string {
  return (
    '<div class="kit-button kit-button--bare">' +
    items.map((b) => `<button type="button" class="${b.variant}" id="${b.id}"${attr('data-writes', b.writes ?? '')}>${esc(b.label)}</button>`).join('') +
    '</div>'
  );
}

/** The kit's Slider (mock/kit/Slider.html), bare, one field. */
function slider(p: { id: string; label: string; value: number }): string {
  const fill = `${p.value}%`;
  return (
    '<div class="kit-slider kit-slider--bare"><div class="kit-slider__field" data-name="volume"><div class="kit-slider__head">' +
    `<label class="kit-slider__label" for="${p.id}">${esc(p.label)}</label>` +
    `<span class="kit-slider__value"><output class="kit-slider__out" for="${p.id}">${p.value}</output><span class="kit-slider__unit">%</span></span></div>` +
    `<input class="kit-slider__input" id="${p.id}" type="range" name="volume" min="0" max="100" value="${p.value}" style="--fill: ${fill}">` +
    '<div class="kit-slider__bounds"><span>0 %</span><span>100 %</span></div></div></div>'
  );
}

const UPLOAD_ICON =
  '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';

/** The kit's FileDrop (mock/kit/FileDrop.html), for the sound files. */
function fileDrop(): string {
  const types = FILE_TYPES.map((t) => t.toUpperCase());
  return (
    '<div class="kit-filedrop is-wide" id="sounds-drop-field"><label class="kit-filedrop__zone" tabindex="0" id="sounds-drop">' +
    UPLOAD_ICON +
    '<span class="kit-filedrop__title">Déposez un son ou <b>parcourez</b></span><span class="kit-filedrop__hint">Effet ponctuel, 30 s maximum</span>' +
    `<span class="kit-filedrop__types">${types.map((t) => `<span class="kit-filedrop__tag">${t}</span>`).join('')}</span>` +
    `<input class="kit-filedrop__input" type="file" id="sounds-file" accept="${FILE_TYPES.map((t) => `.${t}`).join(',')}"></label>` +
    '<div class="kit-filedrop__error">Type de fichier non accepté.</div>' +
    '<div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info"><div class="kit-filedrop__name"></div>' +
    '<div class="kit-filedrop__row"><span class="kit-filedrop__size"></span><span class="kit-filedrop__pct">Prêt</span></div>' +
    '<div class="kit-filedrop__bar"><div class="kit-filedrop__fill"></div></div></div></div></div>'
  );
}

/** The card, its regions empty: what never depends on the sounds — the title, the search, the add form — is there from the first paint. */
export function skeleton(): string {
  const reads = `data-reads="${LIST_SOUNDS}"`;
  return (
    '<div class="mf-sounds layout-2" id="sound-library">' +
    '<section class="panel" id="sounds-list-panel" aria-labelledby="sounds-title">' +
    `<div class="panel__head"><h2 id="sounds-title">Sons de la campagne</h2><span class="small muted" id="sounds-campaign" data-reads="${GET_LIVE_SESSION}"></span><span class="small muted" id="sounds-count" ${reads}></span></div>` +
    `<div id="sounds-search">${input({ id: 'sounds-search-input', label: 'Rechercher', type: 'search', placeholder: 'Nom ou tag : porte, combat, extérieur…' })}</div>` +
    '<div class="row" id="sounds-filter" aria-live="polite"></div>' +
    `<div id="sounds-table" ${reads} aria-live="polite"></div>` +
    '<div id="sounds-status" aria-live="polite"></div>' +
    '</section>' +
    '<aside class="stack" id="sounds-side">' +
    '<form class="panel" id="sounds-upload" aria-labelledby="sounds-upload-title">' +
    '<h2 id="sounds-upload-title">Téléverser un son</h2>' +
    '<p class="small muted">Effets ponctuels uniquement. Le nom sert aussi à la détection au micro : dites « la porte grince » et le son vous est proposé.</p>' +
    '<div id="sounds-upload-fields">' +
    fileDrop() +
    input({ id: 'sounds-new-name', label: 'Nom', placeholder: 'Porte qui grince' }) +
    input({ id: 'sounds-new-tags', label: 'Tags', placeholder: 'porte, bois, intérieur', hint: 'Séparés par des virgules' }) +
    buttons([{ id: 'sounds-new-submit', label: 'Ajouter à la soundbox', variant: 'primary', writes: ADD_SOUND }]) +
    '</div>' +
    '<div id="sounds-upload-result" aria-live="polite"></div>' +
    '</form>' +
    '<form class="panel" id="sounds-edit" aria-labelledby="sounds-edit-title" hidden>' +
    '<h2 id="sounds-edit-title">Modifier le son</h2><div id="sounds-edit-fields"></div><div id="sounds-edit-result" aria-live="polite"></div>' +
    '</form>' +
    '</aside>' +
    '</div>'
  );
}

/** The tag as a button: a click filters the list by it. */
const tagButton = (tag: string, chosen: string | null): string =>
  `<button type="button" data-tag="${esc(tag)}" aria-pressed="${chosen !== null && norm(chosen) === norm(tag)}">#${esc(tag)}</button>`;

/** The kit's Table (mock/kit/Table.html) of the sounds shown. */
export function table(sounds: readonly Sound[], chosen: string | null): string {
  const rows = sounds
    .map(
      (s) =>
        `<tr id="sound-${esc(s.id)}"${s.playing ? ' class="is-playing"' : ''}><td>${esc(s.name)}</td>` +
        `<td><span class="mf-sounds__tags">${s.tags.map((t) => tagButton(t, chosen)).join('')}</span></td>` +
        `<td>${esc(s.file || '—')}</td><td class="num">${duration(s.duration)}</td><td class="num">${s.volume} %</td>` +
        `<td><span class="row"><button type="button" class="chip" data-edit="${esc(s.id)}">Modifier</button></span></td></tr>`,
    )
    .join('');
  return (
    '<table class="kit-table" id="sounds-table-grid"><thead><tr><th>Nom</th><th>Tags</th><th>Fichier</th><th class="num">Durée</th><th class="num">Volume</th><th></th></tr></thead>' +
    `<tbody>${rows}</tbody></table>`
  );
}

/** What stands in place of the table: nothing to list, or nothing that matches. */
export const empty = (library: boolean): string => `<p class="muted" id="sounds-empty">${library ? 'Aucun son dans la bibliothèque : déposez le premier.' : 'Aucun son ne correspond à la recherche.'}</p>`;

export const unreadable = (): string => '<p class="is-error" id="sounds-error">Impossible de lire les sons de la campagne.</p>';

/** The chosen tag, as the kit's Badge, and the way to drop it. */
export function filter(chosen: string | null): string {
  if (chosen === null) return '';
  return (
    `<div class="kit-badge"><span class="primary" id="sounds-filter-badge">Tag : #${esc(chosen)}</span></div>` +
    '<button type="button" class="chip" id="sounds-filter-clear">Retirer le filtre</button>'
  );
}

/** The edit form, opened on one sound: name, tags, own volume, and the two actuators. */
export function editFields(form: EditForm): string {
  return (
    input({ id: 'sounds-edit-name', label: 'Nom', value: form.name }) +
    input({ id: 'sounds-edit-tags', label: 'Tags', value: form.tags, hint: 'Séparés par des virgules' }) +
    slider({ id: 'sounds-edit-volume', label: 'Volume propre', value: form.volume }) +
    buttons([
      { id: 'sounds-edit-save', label: 'Enregistrer', variant: 'primary', writes: UPDATE_SOUND },
      { id: 'sounds-edit-delete', label: 'Supprimer', variant: 'danger', writes: REMOVE_SOUND },
      { id: 'sounds-edit-cancel', label: 'Fermer', variant: 'ghost' },
    ])
  );
}

/** What a result area shows after a command. */
export function result(outcome: Outcome): string {
  if (outcome.kind === 'none') return '';
  if (outcome.kind === 'busy') return `<p class="muted">${esc(outcome.text)}</p>`;
  return `<p class="${outcome.kind === 'ok' ? 'is-ok' : 'is-error'}">${esc(outcome.text)}</p>`;
}
