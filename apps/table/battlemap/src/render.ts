// The Micro-UI's markup, from mock/components/Battlemap.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { GET_BATTLEMAP, GET_BATTLEMAP_INSTANCE, LIST_CHARACTERS, MOVE_TOKEN, PLACE_TOKENS, REMOVE_TOKEN, RENAME_TOKEN, SET_TOKEN_CONDITION, SET_TOKEN_VISIBILITY, SHOW_ON_PLAYER_VIEW } from './ids.ts';
import { center, polygon, trail } from './hexgrid.ts';
import { bandLabel, CONDITIONS, MAX_COUNT, numbering, SCALE } from './model.ts';
import type { Character, Grid, Hex, Instance, MapInfo, Props, Token } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** A colour of the design system by its token name; anything else is no colour. */
const tone = (color: string): string => (/^[a-z][a-z-]*$/.test(color) ? color : 'secondary');

/** The Micro-UI, its regions empty: what never depends on the answers — the frame — is there from the first paint. */
export function skeleton(props: Props): string {
  const modes = `${props.fill ? ' bm--fill' : ''}${props.view === 'player' && !props.pilot ? ' bm--readonly' : ''}`;
  return (
    `<div class="bm${modes}" id="battlemap" data-reads="${GET_BATTLEMAP_INSTANCE}">` +
    '<div class="bm-toolbar" id="battlemap-toolbar"></div>' +
    '<div class="bm-body" id="battlemap-body">' +
    `<aside class="bm-side" id="battlemap-roster" aria-labelledby="battlemap-roster-title" data-reads="${LIST_CHARACTERS}" hidden></aside>` +
    `<div class="bm-map" id="battlemap-map" data-reads="${GET_BATTLEMAP}"></div>` +
    '<aside class="bm-side" id="token-inspector" aria-labelledby="inspector-title" aria-live="polite" hidden></aside>' +
    '</div>' +
    '<p class="bm-status" id="battlemap-status" role="status" aria-live="polite" hidden></p>' +
    '</div>'
  );
}

function button(p: { id: string; label: string; variant: string; writes?: string; disabled?: boolean }): string {
  return `<button type="button" class="${p.variant}" id="${p.id}"${attr('data-writes', p.writes ?? '')}${p.disabled ? ' disabled' : ''}>${esc(p.label)}</button>`;
}

/** The title, what the map holds, and — for the DM — « Envoyer aux joueurs ». */
export function toolbar(map: MapInfo, instance: Instance, props: Props): string {
  if (props.view === 'player') return '';
  const hidden = instance.tokens.filter((t) => !t.visible).length;
  const count = `${instance.tokens.length} token${instance.tokens.length > 1 ? 's' : ''}${hidden > 0 ? ` · ${hidden} caché${hidden > 1 ? 's' : ''}` : ''}`;
  return (
    `<div class="bm-title"><h2 id="battlemap-title">${esc(map.title)}</h2><span class="small muted">${esc(instance.label)} · ${count} · ${SCALE}</span></div>` +
    `<div class="kit-button kit-button--bare kit-button--small">${button({ id: 'bm-send', label: 'Envoyer aux joueurs', variant: 'secondary', writes: SHOW_ON_PLAYER_VIEW })}</div>`
  );
}

function shape(tag: string, attrs: Record<string, string>): string {
  return `<${tag}${Object.entries(attrs).map(([k, v]) => attr(k, v)).join('')}></${tag}>`;
}

/** The kit's HexMap around the image: its terrain, its grid, the overlay a drag draws on, the layer the tokens stand on. */
export function hexMap(map: MapInfo, canMove: boolean): string {
  const box = `viewBox="0 0 ${map.width} ${map.height}" width="${map.width}" height="${map.height}"`;
  const land = map.terrain.length === 0 ? `<rect class="hm-land" x="0" y="0" width="${map.width}" height="${map.height}"></rect>` : '';
  return (
    '<div class="kit-hexmap">' +
    (canMove
      ? '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="Dézoomer">−</button>' +
        '<button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div>'
      : '') +
    `<div class="hm-view" tabindex="0" id="battlemap-view" aria-label="${esc(`Battlemap ${map.title}`)}">` +
    `<div class="hm-stage" id="battlemap-stage" style="width:${map.width}px;height:${map.height}px">` +
    `<svg class="hm-layer hm-map" ${box} aria-hidden="true">${land}${map.terrain.map((s) => shape(s.tag, s.attrs)).join('')}</svg>` +
    `<svg class="hm-layer hm-grid" ${box} aria-hidden="true"><path class="hm-grid-path" id="battlemap-grid" d=""></path></svg>` +
    `<svg class="hm-layer hm-over" id="battlemap-over" ${box}></svg>` +
    `<div class="kit-token bm-tokens" id="battlemap-tokens"${attr('data-writes', canMove ? MOVE_TOKEN : '')}></div>` +
    '</div><div class="hm-tip" id="battlemap-tip" hidden></div></div></div>'
  );
}

/** The kit's Token for one token, as the player or the DM reads it. */
export function tokenItem(t: Token, grid: Grid, p: { view: Props['view']; selected: boolean; character?: Character }): string {
  const player = p.view === 'player';
  const c = center(grid, t);
  const initials = p.character?.initials ?? t.initials;
  const color = p.character?.color || t.color;
  const cls = ['kit-token__item', !t.visible && 'kit-token__item--hidden', t.dead && 'kit-token__item--dead', p.selected && 'bm-selected'].filter(Boolean).join(' ');
  const badges = t.states.slice(0, 3).map((s, i) => `<span class="kit-token__badge kit-token__badge--${['tl', 'tr', 'br'][i]}" title="${esc(s)}">${esc(CONDITIONS[s] ?? '•')}</span>`).join('');
  let health = '';
  if (player) {
    // players-see-band-only: whatever else the answer carries, a player is shown the band.
    if (t.kind === 'pnj' && t.healthBand) health = `<div class="kit-token__hp"><span class="kit-token__hp-label">${esc(bandLabel(t.healthBand))}</span></div>`;
  } else if (t.kind === 'pnj' && t.hp !== null && t.hpMax !== null) {
    const pct = t.hpMax > 0 ? Math.round((100 * Math.max(0, t.hp)) / t.hpMax) : 0;
    const fill = pct <= 25 ? ' kit-token__gauge-fill--low' : pct <= 50 ? ' kit-token__gauge-fill--mid' : '';
    health = `<div class="kit-token__hp"><div class="kit-token__gauge" style="--kit-token-hp: ${pct}%"><div class="kit-token__gauge-fill${fill}"></div></div><span class="kit-token__hp-label">${t.hp}/${t.hpMax} PV</span></div>`;
  }
  const eye =
    '<div class="kit-token__eye"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="Caché"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/><path d="M3 3l18 18"/></svg></div>';
  return (
    `<div class="${cls}" id="token-${esc(t.id)}" data-token="${esc(t.id)}" style="left:${c.x.toFixed(1)}px;top:${c.y.toFixed(1)}px" title="${esc(t.name + (t.visible ? '' : ' — caché aux joueurs'))}">` +
    `<div class="kit-token__disc" style="--kit-token-bg: var(--color-${tone(color)})"><div class="kit-token__clip"><span class="kit-token__initials">${esc(initials)}</span></div>` +
    `${t.visible ? '' : eye}${t.dead ? '<span class="kit-token__strike"></span>' : ''}${badges}` +
    `${t.num === null ? '' : `<span class="kit-token__num">${t.num}</span>`}</div>` +
    `<div class="kit-token__name">${esc(t.name)}</div>${health}</div>`
  );
}

/** The path a carried token would follow, over the overlay: every hex a step, the last one the end. */
export function pathMarkup(grid: Grid, hexes: Hex[]): string {
  return hexes.map((h, i) => `<polygon class="${i === hexes.length - 1 ? 'hm-end' : 'hm-step'}" points="${polygon(grid, h)}"></polygon>`).join('') + `<polyline class="hm-line" points="${trail(grid, hexes)}"></polyline>`;
}

/** The token's menu: its name, whether the players see it, its conditions, its removal. */
export function inspector(t: Token, siblings: readonly Token[], character: Character | undefined): string {
  const chips =
    siblings.length > 1
      ? `<div class="row" id="inspector-instances" role="group" aria-label="Instances de ${esc(character?.name ?? t.name)}">${siblings
          .map((x) => `<button type="button" class="chip${x.id === t.id ? ' is-on' : ''}" data-select="${esc(x.id)}">${esc(x.name)}${x.dead ? ' †' : ''}</button>`)
          .join('')}</div>`
      : '';
  const conditions = Object.entries(CONDITIONS)
    .map(([name, glyph]) => `<button type="button" class="chip${t.states.includes(name) ? ' is-on' : ''}" data-state="${esc(name)}" aria-pressed="${t.states.includes(name)}" data-writes="${SET_TOKEN_CONDITION}">${glyph} ${esc(name)}</button>`)
    .join('');
  return (
    `<div class="panel__head"><h3 id="inspector-title">${esc(t.name)}</h3><button type="button" class="icon-btn" data-act="close" aria-label="Fermer le menu du token">×</button></div>` +
    `<p class="small muted">${esc(character?.name ?? '')}</p>` +
    chips +
    `<div class="kit-input is-wide"><label>Nom affiché<input type="text" id="inspector-name" value="${esc(t.name)}" data-writes="${RENAME_TOKEN}"></label></div>` +
    `<div class="kit-toggle kit-toggle--bare"><label><button type="button" role="switch" id="inspector-visible" aria-checked="${t.visible}" data-writes="${SET_TOKEN_VISIBILITY}"><i></i></button><span>${t.visible ? 'Visible des joueurs' : 'Caché aux joueurs'}</span></label></div>` +
    `<div id="inspector-states" class="stack"><h3>États</h3><div class="row">${conditions}</div></div>` +
    (t.dead ? '<p class="bm-dead" id="inspector-dead">Mort — grisé.</p>' : '') +
    `<div class="kit-button kit-button--bare kit-button--small">${button({ id: 'inspector-remove', label: t.dead ? 'Retirer le corps' : 'Retirer de la carte', variant: 'danger', writes: REMOVE_TOKEN })}</div>`
  );
}

/** The characters of the scene, to drag onto the map or to pick. */
export function roster(characters: readonly Character[]): string {
  return (
    '<h3 id="battlemap-roster-title">Personnages de la scène</h3>' +
    '<p class="small muted">Glissez un personnage sur la carte, ou cliquez pour choisir le nombre d’instances (× N).</p>' +
    `<ul class="list" id="battlemap-roster-list">${characters
      .map((c) => `<li><button type="button" class="chip chip--${c.kind}" draggable="true" data-writes="${PLACE_TOKENS}" data-char="${esc(c.id)}" id="roster-${esc(c.id)}">⠿ ${esc(c.name)}</button></li>`)
      .join('')}</ul>`
  );
}

/** « Placer Bandit » : how many, and what the numbers will be; a PC has one token. */
export function dropForm(character: Character, tokens: readonly Token[], count: number): string {
  return (
    `<b>Placer ${esc(character.name)}</b>` +
    (character.kind === 'pnj'
      ? `<label class="row small">× <input type="number" class="num-input" id="battlemap-drop-count" min="1" max="${MAX_COUNT}" value="${count}"> instance(s)</label>` +
        `<span class="small muted" id="battlemap-drop-numbers">${esc(numbering(tokens, character, count))}</span>`
      : '<span class="small muted">Un PJ n’a qu’un token sur la carte.</span>') +
    `<div class="kit-button kit-button--bare kit-button--small">${button({ id: 'battlemap-drop-ok', label: 'Placer', variant: 'primary', writes: PLACE_TOKENS })}${button({ id: 'battlemap-drop-cancel', label: 'Annuler', variant: 'ghost' })}</div>`
  );
}
