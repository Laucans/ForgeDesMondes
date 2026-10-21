// The card's markup, from mock/components/MediaLibrary.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { ADD_PLACE_TO_SESSION, READS, LIST_PLACES, SET_MEDIA_PLACE, UPLOAD_MEDIA } from './ids.ts';
import { association } from './model.ts';
import type { Draft, Kind, Medium, Place } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** The kit's Button, one ghost, secondary or primary button. */
function button(p: { id?: string; label: string; variant: string; small?: boolean; action?: string; writes?: string; title?: string }): string {
  return (
    `<div class="kit-button kit-button--bare${p.small ? ' kit-button--small' : ''}">` +
    `<button type="button" class="${p.variant}"${attr('id', p.id ?? '')}${attr('data-action', p.action ?? '')}${attr('data-writes', p.writes ?? '')}${attr('title', p.title ?? '')}>${esc(p.label)}</button></div>`
  );
}

/** The kit's Select, dressed on a native <select>: a field a browser and a screen reader know as a list of options. */
function select(p: { id: string; label: string; value: string; options: readonly Place[]; none: string; writes?: string; reads?: string }): string {
  const id = esc(p.id);
  const options = [{ id: '', name: p.none }, ...p.options].map((o) => `<option value="${esc(o.id)}"${o.id === p.value ? ' selected' : ''}>${esc(o.name)}</option>`).join('');
  return (
    `<div class="kit-select is-wide"><span id="${id}-label">${esc(p.label)}</span>` +
    `<select id="${id}" aria-labelledby="${id}-label"${attr('data-writes', p.writes ?? '')}${attr('data-reads', p.reads ?? '')}>${options}</select></div>`
  );
}

/** The card's frame: the title, the upload form, the two lists, empty. */
export function skeleton(): string {
  return (
    '<section class="panel mf-media" id="media-library" aria-labelledby="media-title">' +
    `<div class="panel__head"><h2 id="media-title">Bibliothèque de médias</h2><span class="small muted" id="media-count" data-reads="${READS}"></span></div>` +
    '<form class="mf-media__upload" id="media-upload">' +
    `<div id="media-upload-drop">${fileDrop()}</div>` +
    `<div class="stack" id="media-upload-fields" data-reads="${LIST_PLACES}"></div>` +
    '</form>' +
    '<div class="mf-media__message" id="media-message" aria-live="polite"></div>' +
    '<section id="media-battlemaps" aria-labelledby="media-battlemaps-title" class="stack"><h3 id="media-battlemaps-title">Battlemaps</h3>' +
    `<ul class="mf-media__grid" id="media-battlemaps-list" data-reads="${READS}"></ul></section>` +
    '<section id="media-illustrations" aria-labelledby="media-illustrations-title" class="stack"><h3 id="media-illustrations-title">Illustrations</h3>' +
    `<ul class="mf-media__grid" id="media-illustrations-list" data-reads="${READS}"></ul></section>` +
    '</section>'
  );
}

const ICON_UPLOAD =
  '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';

export const TYPES = ['JPG', 'PNG', 'WEBP'];

/** The kit's FileDrop: the zone, the type error, and the chosen file with its size. */
export function fileDrop(): string {
  return (
    '<div class="kit-filedrop is-wide" id="media-upload-drop-zone">' +
    `<label class="kit-filedrop__zone" tabindex="0" id="media-upload-zone">${ICON_UPLOAD}` +
    '<span class="kit-filedrop__title">Déposez une image ou <b>parcourez</b></span>' +
    '<span class="kit-filedrop__hint">Battlemap préconstruite ou illustration, jusqu’à 25 Mo</span>' +
    `<span class="kit-filedrop__types">${TYPES.map((t) => `<span class="kit-filedrop__tag">${t}</span>`).join('')}</span>` +
    `<input class="kit-filedrop__input" id="media-upload-file" type="file" accept="${TYPES.map((t) => `.${t.toLowerCase()}`).join(',')},.jpeg"></label>` +
    '<div class="kit-filedrop__error">Type de fichier non accepté.</div>' +
    '<div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info">' +
    '<div class="kit-filedrop__name"></div><div class="kit-filedrop__row"><span class="kit-filedrop__size"></span></div></div></div>' +
    '</div>'
  );
}

/** The upload form's other fields: the kind, the optional place, the title, and the button. */
export function uploadFields(draft: Draft, places: readonly Place[]): string {
  const radio = (kind: Kind, label: string, hint: string): string =>
    `<label><input type="radio" name="media-kind" value="${kind}"${draft.kind === kind ? ' checked' : ''}><span>${label}</span><small>${hint}</small></label>`;
  return (
    '<div class="kit-radio"><fieldset id="media-upload-kind"><legend>Type</legend>' +
    radio('battlemap', 'Battlemap', 'grille hexagonale') +
    radio('illustration', 'Illustration', 'scène ou fenêtre') +
    '</fieldset></div>' +
    select({ id: 'media-upload-place', label: 'Lieu (facultatif)', value: draft.place, options: places, none: 'Aucun lieu' }) +
    `<div class="kit-input is-wide"><label>Titre<input type="text" id="media-upload-title" placeholder="La cave du Sanglier"${attr('value', draft.title)}></label></div>` +
    button({ id: 'media-upload-submit', label: 'Ajouter à la campagne', variant: 'primary', writes: UPLOAD_MEDIA })
  );
}

const ratio = (value: number): string => String(value).replace('.', ',');

/** The line under the title: the file, its size, and the grid or the ratio. */
function caption(m: Medium): string {
  // The answer carries no weight: a battlemap says its pixel size instead, an illustration its ratio alone.
  const weight = m.size !== '' ? m.size : m.width !== null && m.height !== null ? `${m.width} × ${m.height} px` : '';
  const grid = m.kind === 'battlemap' ? (m.grid === null ? '' : `grille ${m.grid.orient === 'pointy' ? 'pointe' : 'côté plat'} ${m.grid.size} px`) : m.ratio === null ? '' : `ratio ${ratio(m.ratio)}`;
  return [m.file, weight, grid].filter((part) => part !== '').join(' · ');
}

/** « Associé à la séance 4 », or where the medium stands: no place, a place outside the session, a place in it. */
function placeLink(m: Medium, places: readonly Place[], sessionPlaces: ReadonlySet<string> | null, sessionNumber: number | null): string {
  const standing = association(m, sessionPlaces);
  const id = `id="media-session-${esc(m.id)}"`;
  const number = sessionNumber === null ? '' : ` ${sessionNumber}`;
  const name = (place: string): string => places.find((p) => p.id === place)?.name ?? place;
  switch (standing.kind) {
    case 'in':
      return `<small ${id}><span class="chip chip--lieu">Associé à la séance${number}</span> via ${esc(name(standing.place))}</small>`;
    case 'out':
      return (
        `<small class="muted" ${id}>Lieu hors de la séance${number}</small>` +
        button({ label: 'Ajouter le lieu à la séance', variant: 'ghost', small: true, action: `add-place:${standing.place}`, writes: ADD_PLACE_TO_SESSION, title: `Ajouter « ${name(standing.place)} » à la séance${number}` })
      );
    case 'none':
      return sessionPlaces === null ? '' : `<small class="muted" ${id}>Sans lieu : non rattaché à la séance</small>`;
  }
}

/** The places the select offers; a place the campaign does not list stays selectable, by its id, rather than showing « Aucun lieu ». */
const withPlace = (places: readonly Place[], place: string | null): readonly Place[] => (place === null || places.some((p) => p.id === place) ? places : [...places, { id: place, name: place }]);

/** One medium's card. */
export function card(m: Medium, places: readonly Place[], sessionPlaces: ReadonlySet<string> | null, sessionNumber: number | null, selected: boolean): string {
  return (
    `<li class="mf-media__item${selected ? ' is-selected' : ''}" id="media-${esc(m.id)}" data-id="${esc(m.id)}">` +
    `<div class="mf-media__thumb"><div class="art art--${esc(m.art)}" role="img" aria-label="${esc(m.title)}"><span class="art__glyph">${esc(m.glyph || m.title.charAt(0).toUpperCase())}</span></div></div>` +
    `<b>${esc(m.title)}</b><small>${esc(caption(m))}</small>` +
    select({ id: `media-place-${m.id}`, label: 'Lieu', value: m.place ?? '', options: withPlace(places, m.place), none: 'Aucun lieu', writes: SET_MEDIA_PLACE }) +
    placeLink(m, places, sessionPlaces, sessionNumber) +
    (m.kind === 'battlemap' ? button({ label: 'Régler la grille', variant: selected ? 'primary' : 'secondary', small: true, action: `grid:${m.id}` }) : '') +
    '</li>'
  );
}

/** « 2 battlemaps · 3 illustrations ». */
export function count(battlemaps: number, illustrations: number): string {
  const plural = (n: number, one: string): string => `${n} ${one}${n > 1 ? 's' : ''}`;
  return `${plural(battlemaps, 'battlemap')} · ${plural(illustrations, 'illustration')}`;
}

export const message = (kind: 'ok' | 'error' | 'busy', words: string): string => `<p class="is-${kind}" id="media-message-text">${esc(words)}</p>`;
