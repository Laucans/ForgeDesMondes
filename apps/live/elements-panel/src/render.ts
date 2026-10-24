// The panel's markup, from mock/components/ElementsPanel.html: the same ids,
// the same classes. Every part names the unit it shows (`data-reads`) or
// triggers (`data-writes`) by identifier.

import { LIST_CHARACTERS, LIST_EVENTS, LIST_MEDIA, LIST_PLACES, LIST_SCENES, SHOW_ON_PLAYER_VIEW, SHOW_WINDOW_ON_PLAYER_VIEW } from './ids.ts';
import { normalize } from './model.ts';
import type { Group, Item, Scene } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** The panel, its regions empty: the search box (the kit's Input) stays where it is whatever is read again. */
export function skeleton(): string {
  return (
    '<nav class="mf-ep" id="elements-panel" aria-label="Éléments de la campagne">' +
    '<div id="elements-search"><div class="kit-input"><label>Rechercher<input type="search" id="elements-search-input" placeholder="Nom ou alias, même approximatif" autocomplete="off"></label></div></div>' +
    '<p class="mf-ep__status" id="elements-status" role="status" hidden></p>' +
    '<section id="elements-results" aria-labelledby="elements-results-title" hidden><h2 id="elements-results-title">Résultats</h2><ul class="mf-ep__list" id="elements-results-list"></ul></section>' +
    '<section id="elements-session" aria-labelledby="elements-session-title"><h2 id="elements-session-title">Séance en cours</h2>' +
    `<div id="elements-scenes" data-reads="${LIST_SCENES} ${LIST_CHARACTERS} ${LIST_MEDIA}"></div></section>` +
    '<section id="elements-campaign" aria-labelledby="elements-campaign-title"><h2 id="elements-campaign-title">Toute la campagne</h2>' +
    `<div id="elements-groups" data-reads="${LIST_CHARACTERS} ${LIST_PLACES} ${LIST_EVENTS} ${LIST_MEDIA}"></div></section>` +
    '</nav>'
  );
}

/** The chips a battlemap or an illustration carries. */
function actions(item: Item): string {
  if (item.type === 'battlemap') {
    if (item.instance === undefined) return '';
    const instance = esc(item.instance);
    return `<button type="button" class="chip" data-open-map="${instance}">Ouvrir</button><button type="button" class="chip" data-writes="${SHOW_ON_PLAYER_VIEW}" data-send-map="${instance}">Envoyer</button>`;
  }
  if (item.type === 'illustration') {
    const id = esc(item.id);
    return (
      `<button type="button" class="chip" data-writes="${SHOW_ON_PLAYER_VIEW}" data-send-scene="${id}" title="Envoyer en plein écran">Scène</button>` +
      `<button type="button" class="chip" data-writes="${SHOW_WINDOW_ON_PLAYER_VIEW}" data-send-window="${id}" title="Envoyer dans une fenêtre flottante">Fenêtre</button>`
    );
  }
  return '';
}

/** The button that names an element: it opens the sheet, or the battlemap when the session holds an instance of it. */
function opener(item: Item): string {
  const label = `${esc(item.name)}${item.kind === 'pj' ? '<small>PJ</small>' : ''}`;
  if (item.type === 'battlemap') {
    return item.instance === undefined
      ? `<button type="button" class="mf-ep__open" disabled title="Aucune instance de cette carte dans la séance">${label}</button>`
      : `<button type="button" class="mf-ep__open" data-open-map="${esc(item.instance)}">${label}</button>`;
  }
  return `<button type="button" class="mf-ep__open" data-open="${esc(item.type)}:${esc(item.id)}">${label}</button>`;
}

function row(item: Item, prefix: string): string {
  const drag = item.type === 'character';
  return (
    `<li class="mf-ep__item" id="${esc(`${prefix}-${item.type}-${item.id}`)}"${drag ? ` draggable="true" data-char="${esc(item.id)}"` : ''}>` +
    `${drag ? '<span class="mf-ep__grip" aria-hidden="true">⠿</span>' : ''}${opener(item)}${actions(item)}</li>`
  );
}

export const list = (items: readonly Item[], prefix: string): string => `<ul class="mf-ep__list">${items.map((item) => row(item, prefix)).join('')}</ul>`;

/** The search's results, one row each. */
export const results = (items: readonly Item[]): string => items.map((item) => row(item, 'found')).join('');

/** The session's scenes, one `details` each in the order given; the one the session is at (the first, to begin with) is open. */
export function scenes(all: readonly Scene[], groupsOf: (scene: Scene) => Group[], current: number): string {
  if (all.length === 0) return '<p class="mf-ep__empty">Aucune scène dans la séance.</p>';
  return all
    .map((scene, i) => {
      const groups = groupsOf(scene)
        .map((g) => `<div class="mf-ep__group"><h3>${esc(g.label)}</h3>${list(g.items, scene.id)}</div>`)
        .join('');
      return `<details id="elements-scene-${esc(scene.id)}" data-scene-index="${i}"${i === current ? ' open class="is-current"' : ''}><summary>Scène ${i + 1} · ${esc(scene.title)}</summary>${groups}</details>`;
    })
    .join('');
}

/** The whole campaign, one `details` a type. */
export const groups = (all: readonly Group[]): string =>
  all.map((g) => `<details id="elements-group-${esc(normalize(g.label))}"><summary>${esc(g.label)} <small>${g.items.length}</small></summary><div class="mf-ep__group">${list(g.items, 'all')}</div></details>`).join('');
