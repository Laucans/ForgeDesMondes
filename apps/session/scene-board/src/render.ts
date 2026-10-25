// The board's markup, from mock/components/SceneBoard.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { ADD_PRESENT_CHARACTER, ADD_SCENE, CREATE_BATTLEMAP_INSTANCE, GET_BATTLEMAP_INSTANCE, GET_SESSION, LIST_CHARACTERS, LIST_EVENTS, LIST_MEDIA, LIST_PLACES, LIST_SCENES, REMOVE_PRESENT_CHARACTER, REORDER_SCENES, UPDATE_SCENE_NOTES } from './ids.ts';
import { absentPcs, pcs, presentPcs } from './model.ts';
import type { Board, Scene } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** One of the kit's Buttons (mock/kit/Button.html), its extra attributes given by the caller. */
const button = (attrs: string, label: string, variant: string): string => `<button type="button" class="${variant}"${attrs}>${esc(label)}</button>`;
/** The kit's Button group, small. */
const buttons = (inner: string): string => `<div class="kit-button kit-button--bare kit-button--small">${inner}</div>`;

/** The board, its regions empty: what never depends on the answers — the titles, « Ajouter une scène » — is there from the first paint. */
export function skeleton(): string {
  return (
    `<section class="mf-scenes stack" id="scene-board" aria-labelledby="scenes-title" data-reads="${GET_SESSION}">` +
    '<p class="mf-scenes__notice" id="scenes-notice" aria-live="polite"></p>' +
    '<section class="panel" id="scenes-present" aria-labelledby="scenes-present-title">' +
    '<div class="panel__head"><h2 id="scenes-present-title">PJ présents</h2><span class="small muted" id="scenes-present-count"></span></div>' +
    `<div class="row" id="scenes-present-list" data-reads="${GET_SESSION} ${LIST_CHARACTERS}"></div>` +
    '<div class="row" id="scenes-present-add"></div>' +
    '</section>' +
    '<section class="panel" id="scenes-list-panel" aria-labelledby="scenes-title">' +
    `<div class="panel__head"><h2 id="scenes-title">Scènes</h2>${buttons(button(` id="scenes-add" data-writes="${ADD_SCENE}"`, 'Ajouter une scène', 'secondary'))}</div>` +
    `<ol class="mf-scenes__list" id="scenes-list" data-reads="${LIST_SCENES} ${LIST_PLACES} ${LIST_EVENTS} ${LIST_MEDIA}"></ol>` +
    '</section></section>'
  );
}

/** « Scènes de la séance 4 — L’embuscade du Gué », once the session is known. */
export function sceneTitle(board: Board): string {
  const { order, title } = board.session;
  return ['Scènes', order === null ? '' : `de la séance ${order}`, title === '' ? '' : `— ${title}`].filter((part) => part !== '').join(' ');
}

export const presentCount = (board: Board): string => `${presentPcs(board).length} sur ${pcs(board).length}`;

/** The PCs ticked present, each with its « × ». */
export function presentList(board: Board): string {
  return presentPcs(board)
    .map(
      (pc) =>
        `<span class="chip chip--pj" id="scenes-present-${esc(pc.id)}">${esc(pc.name)}` +
        `<button type="button" class="icon-btn" data-writes="${REMOVE_PRESENT_CHARACTER}" data-remove-pj="${esc(pc.id)}" aria-label="Retirer ${esc(pc.name)}">×</button></span>`,
    )
    .join('');
}

/** The PCs of the campaign still to tick, as a list to pick from and « Ajouter »; nothing once all are present. */
export function presentAdd(board: Board): string {
  const absent = absentPcs(board);
  if (absent.length === 0) return '';
  return (
    '<div class="kit-select"><span id="scenes-present-label">Ajouter un PJ</span>' +
    `<select id="scenes-present-select" aria-labelledby="scenes-present-label">${absent.map((pc) => `<option value="${esc(pc.id)}">${esc(pc.name)}</option>`).join('')}</select></div>` +
    buttons(button(` id="scenes-present-submit" data-writes="${ADD_PRESENT_CHARACTER}"`, 'Ajouter', 'secondary'))
  );
}

const link = (cls: string, href: string, label: string, id: string): string => `<a class="chip chip--${cls}" href="${esc(href)}" id="${esc(id)}">${esc(label)}</a>`;

/** What a scene links to. A name no answer gives stays its id, as the mock-up draws an entity it does not know. */
function chips(scene: Scene, board: Board): string {
  const character = (id: string): string => board.characters.find((c) => c.id === id)?.name ?? id;
  const place = (id: string): string => board.places.find((p) => p.id === id)?.name ?? id;
  const event = (id: string): string => board.events.find((e) => e.id === id)?.name ?? id;
  const mediaTitle = (id: string): string => [...board.medias.battlemaps, ...board.medias.illustrations].find((m) => m.id === id)?.title ?? id;
  const rows: Array<[string, string[]]> = [
    ['Lieux', scene.places.map((id) => link('lieu', `fiche-entite.html?id=${encodeURIComponent(id)}`, place(id), `${scene.id}-place-${id}`))],
    ['PNJ', scene.characters.map((id) => link('pnj', `fiche-entite.html?id=${encodeURIComponent(id)}`, character(id), `${scene.id}-char-${id}`))],
    ['Événements', scene.events.map((id) => link('media', `fiche-entite.html?id=${encodeURIComponent(id)}`, event(id), `${scene.id}-event-${id}`))],
    ['Médias', scene.media.map((id) => link('media', 'medias.html', mediaTitle(id), `${scene.id}-media-${id}`))],
    ['À créer', scene.toCreate.map((id) => link('todo', `#to-create-${id}`, id, `${scene.id}-todo-${id}`))],
  ];
  const shown = rows.filter(([, items]) => items.length > 0);
  return shown.length === 0 ? '' : `<dl class="mf-scenes__links">${shown.map(([name, items]) => `<dt>${name}</dt><dd>${items.join('')}</dd>`).join('')}</dl>`;
}

const notes = (scene: Scene): string =>
  `<div class="kit-textarea is-wide"><label class="kit-textarea__label" for="scene-${esc(scene.id)}-notes">Notes</label>` +
  `<textarea class="kit-textarea__field" id="scene-${esc(scene.id)}-notes" rows="2" data-writes="${UPDATE_SCENE_NOTES}">${esc(scene.notes)}</textarea>` +
  '<div class="kit-textarea__footer"><p class="kit-textarea__hint"></p></div></div>';

const plural = (n: number, one: string, many: string): string => (n > 1 ? many : one);

/** The scene's battlemap: its instance when it has one — never a second to create — else the campaign's battlemaps to make one from. */
function mapLine(scene: Scene, board: Board): string {
  const id = `${esc(scene.id)}-battlemap`;
  if (scene.battlemap !== null) {
    const instance = board.instances[scene.battlemap];
    const map = instance === undefined ? undefined : board.medias.battlemaps.find((m) => m.id === instance.battlemap);
    const hidden = instance !== undefined && instance.hidden > 0 ? ` dont ${instance.hidden} ${plural(instance.hidden, 'caché', 'cachés')}` : '';
    const detail = instance === undefined ? 'instance de cette scène' : `instance de cette scène · ${instance.tokens} ${plural(instance.tokens, 'token placé', 'tokens placés')}${hidden}`;
    return (
      `<div class="mf-scenes__map is-linked" id="${id}" data-reads="${GET_BATTLEMAP_INSTANCE}">` +
      `<span><b>${esc(map?.title ?? scene.battlemap)}</b> <span class="small muted">· ${esc(detail)}</span></span>` +
      buttons(button(' data-action="place"', 'Placer les personnages', 'secondary')) +
      '</div>'
    );
  }
  const maps = board.medias.battlemaps;
  const create =
    maps.length === 0
      ? '<span class="small muted">La campagne n’a pas de battlemap à associer.</span>'
      : '<div class="row">' +
        `<div class="kit-select"><span id="${id}-label">Battlemap</span><select id="${id}-select" data-map-select aria-labelledby="${id}-label">${maps.map((m) => `<option value="${esc(m.id)}">${esc(m.title)}</option>`).join('')}</select></div>` +
        buttons(button(` data-create-instance data-writes="${CREATE_BATTLEMAP_INSTANCE}"`, 'Créer l’instance', 'secondary')) +
        '</div>';
  return (
    `<div class="mf-scenes__map" id="${id}"><span class="muted small">Aucune battlemap pour cette scène</span>${create}` +
    '<span class="small muted">Une nouvelle instance de la carte est créée pour cette scène : ses tokens et son combat lui sont propres.</span></div>'
  );
}

/** The scenes in order, each with the arrows that move it, its links, its notes and its battlemap. */
export function sceneList(board: Board): string {
  const last = board.scenes.length - 1;
  return board.scenes
    .map(
      (scene, index) =>
        `<li class="mf-scenes__scene" id="scene-${esc(scene.id)}" data-scene="${esc(scene.id)}">` +
        `<div class="mf-scenes__order"><button type="button" class="icon-btn" data-writes="${REORDER_SCENES}" data-move="-1" aria-label="Monter"${index === 0 ? ' disabled' : ''}>▲</button>` +
        `<b>${index + 1}</b><button type="button" class="icon-btn" data-writes="${REORDER_SCENES}" data-move="1" aria-label="Descendre"${index === last ? ' disabled' : ''}>▼</button></div>` +
        `<div class="mf-scenes__body"><h3>${esc(scene.title)}</h3>${chips(scene, board)}${notes(scene)}${mapLine(scene, board)}</div></li>`,
    )
    .join('');
}
