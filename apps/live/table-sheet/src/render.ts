// The sheet's markup, from mock/components/TableSheet.html: the same ids, the
// same mf-sheet classes, the kit's Badge, Button and StatBlock. Every part
// names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier. Nothing here is a field: the sheet is read, never edited in
// place; Modifier is a link to the entity editor.

import { EDITOR, GET_ENTITY, LIST_MEDIA, SHOW_ON_PLAYER_VIEW, SHOW_WINDOW_ON_PLAYER_VIEW } from './ids.ts';
import { modifierText } from './model.ts';
import { ABILITIES, type Entity, type Illustration, type Ref } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** An image's placeholder: its palette name and glyph, named for a screen reader. */
export function art(image: { art: string; glyph: string; title: string }): string {
  const name = /^[a-z]+$/.test(image.art) ? image.art : 'night';
  return `<div class="art art--${name}" role="img" aria-label="${esc(image.title)}">${image.glyph === '' ? '' : `<span class="art__glyph">${esc(image.glyph)}</span>`}</div>`;
}

type Tone = 'primary' | 'success' | 'accent' | 'danger';

/** What the sheet draws of an element, once read. */
export interface View {
  id: string;
  title: string;
  aliases: string[];
  badges: Array<{ label: string; tone: Tone }>;
  picture: { art: string; glyph: string; title: string };
  /** The markup of the text column. */
  text: string;
  /** The markup of the side column. */
  side: string;
  /** The entity editor opens for this id; none for an image. */
  editable: boolean;
  /** The illustration the send buttons name; none, no button. */
  sends: Illustration | undefined;
  /** A character can be placed on the map. */
  toMap: boolean;
}

/** The sheet, its regions empty: a status line says what a send did. */
export function skeleton(): string {
  return (
    `<article class="mf-sheet" id="table-sheet" aria-labelledby="sheet-title" data-reads="${GET_ENTITY}">` +
    `<header class="mf-sheet__head" id="sheet-head" data-reads="${LIST_MEDIA}"></header>` +
    '<div class="mf-sheet__body" id="sheet-body"></div>' +
    '<p class="mf-sheet__status" id="sheet-status" role="status" hidden></p>' +
    '</article>'
  );
}

/** What the sheet says in place of an element: nothing open, none found, not readable. */
export function notice(id: string, message: string): string {
  return `<p class="muted" id="${id}" role="status">${esc(message)}</p>`;
}

function badges(items: View['badges']): string {
  return `<div class="kit-badge">${items.map((b) => `<span class="${b.tone}">${esc(b.label)}</span>`).join('')}</div>`;
}

/** The kit's Button, small and bare: the editor's link, then the buttons that send or place. */
function buttons(view: View): string {
  const items: string[] = [];
  if (view.editable) items.push(`<a class="ghost" id="sheet-edit" href="${EDITOR}?id=${encodeURIComponent(view.id)}">Modifier</a>`);
  if (view.toMap) items.push('<button type="button" class="secondary" id="sheet-to-map" data-action="to-map">Placer sur la carte</button>');
  if (view.sends !== undefined) {
    const media = esc(view.sends.id);
    items.push(`<button type="button" class="secondary" id="sheet-send-scene" data-action="send-scene" data-media="${media}" data-writes="${SHOW_ON_PLAYER_VIEW}">Envoyer l’illustration — scène</button>`);
    items.push(`<button type="button" class="ghost" id="sheet-send-window" data-action="send-window" data-media="${media}" data-writes="${SHOW_WINDOW_ON_PLAYER_VIEW}">Envoyer — fenêtre</button>`);
  }
  return `<div class="kit-button kit-button--bare kit-button--small">${items.join('')}</div>`;
}

/** The head and the body of one element, as the sheet shows them. */
export function sheet(view: View): { head: string; body: string } {
  const head =
    `<div class="mf-sheet__art">${art(view.picture)}</div>` +
    `<div class="mf-sheet__title">${badges(view.badges)}<h2 id="sheet-title">${esc(view.title)}</h2>` +
    (view.aliases.length > 0 ? `<p class="muted">Alias : ${esc(view.aliases.join(', '))}</p>` : '') +
    `${buttons(view)}</div>`;
  const body = `<div class="mf-sheet__text" id="sheet-text">${view.text}</div><div id="sheet-side">${view.side}</div>`;
  return { head, body };
}

const paragraph = (value: string): string => (value === '' ? '' : `<p>${esc(value)}</p>`);

const vital = (label: string, value: string): string => `<div class="kit-statblock__vital"><dt>${label}</dt><dd>${esc(value === '' ? '—' : value)}</dd></div>`;

/** The kit's StatBlock of a character: CA, PV and speed, the six abilities with their modifiers, its features. */
function statBlock(entity: Entity): string {
  const abilities = ABILITIES.map((k) => {
    const score = entity.stats[k];
    return `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${k}</span><span class="kit-statblock__score">${score === null ? '—' : score}</span><span class="kit-statblock__mod">${modifierText(score, entity.modifiers[k])}</span></li>`;
  }).join('');
  const features = entity.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join('');
  return (
    '<div class="kit-statblock" id="sheet-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body">' +
    `<h3 class="kit-statblock__name">${esc(entity.name)}</h3><p class="kit-statblock__type">${esc(entity.creature)}</p>` +
    `<dl class="kit-statblock__vitals">${vital('CA', entity.ca === null ? '' : String(entity.ca))}${vital('PV', entity.pvMax === null ? '' : String(entity.pvMax))}${vital('Vitesse', entity.speed)}</dl>` +
    `<ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${features}</ul></div></div>`
  );
}

/** The characters a place lets the DM meet, each opening its own sheet on a click. */
function meetings(refs: readonly Ref[]): string {
  if (refs.length === 0) return '';
  const chips = refs.map((c) => `<button type="button" class="chip chip--${c.pc ? 'pj' : 'pnj'}" data-open="character:${esc(c.id)}">${esc(c.name)}</button>`).join('');
  return `<div class="mf-sheet__block" id="sheet-place-characters"><h3>On y croise</h3><div class="row">${chips}</div></div>`;
}

/** The view of an element read by campaign.getEntity; `met` are a place's characters, read for their names. */
export function viewOf(entity: Entity, illustration: Illustration | undefined, met: readonly Ref[]): View {
  const base = { id: entity.id, title: entity.name, aliases: entity.aliases, editable: true, sends: illustration };
  if (entity.type === 'character') {
    const text =
      paragraph(entity.description) +
      (entity.behavior === '' ? '' : `<div class="mf-sheet__block mf-sheet__play" id="sheet-behavior"><h3>Pour l’interpréter</h3><p>${esc(entity.behavior)}</p></div>`) +
      (entity.background === '' ? '' : `<details id="sheet-background"><summary>Background</summary><p>${esc(entity.background)}</p></details>`) +
      (entity.notes === '' ? '' : `<div class="mf-sheet__block mf-sheet__notes" id="sheet-notes"><h3>Notes du MJ</h3><p>${esc(entity.notes)}</p></div>`);
    return { ...base, badges: [{ label: entity.pc ? 'PJ' : 'PNJ', tone: 'primary' }], picture: { art: entity.art, glyph: entity.initials, title: entity.name }, text, side: statBlock(entity), toMap: true };
  }
  if (entity.type === 'place') {
    const picture = illustration === undefined ? { art: entity.art, glyph: entity.name.charAt(0), title: entity.name } : { art: illustration.art, glyph: illustration.glyph, title: entity.name };
    return { ...base, badges: [{ label: 'Lieu', tone: 'accent' }], picture, text: paragraph(entity.description), side: meetings(met), toMap: false };
  }
  return { ...base, badges: [{ label: 'Événement', tone: 'danger' }], picture: { art: 'blood', glyph: '!', title: entity.name }, text: paragraph(entity.description), side: '', toMap: false };
}

/** The view of an image of the library opened as an element: no editor, only the sends. */
export function viewOfIllustration(image: Illustration): View {
  const details = [image.file, image.size].filter((part) => part !== '').join(' · ');
  return {
    id: image.id,
    title: image.title,
    aliases: [],
    badges: [{ label: 'Illustration', tone: 'accent' }],
    picture: { art: image.art, glyph: image.glyph, title: image.title },
    text: details === '' ? '' : `<p class="muted">${esc(details)}</p>`,
    side: '',
    editable: false,
    sends: image,
    toMap: false,
  };
}
