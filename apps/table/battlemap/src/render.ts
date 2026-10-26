// The Micro-UI's markup, from mock/components/Battlemap.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import {
  ADD_PARTICIPANT,
  CHANGE_HIT_POINTS,
  END_COMBAT,
  GET_BATTLEMAP,
  GET_BATTLEMAP_INSTANCE,
  GET_DIFFUSION,
  LIST_CHARACTERS,
  MOVE_TOKEN,
  NEXT_TURN,
  PLACE_TOKENS,
  REMOVE_PARTICIPANT,
  REMOVE_TOKEN,
  RENAME_TOKEN,
  REORDER_INITIATIVE,
  RESET_BATTLEMAP,
  SET_TOKEN_CONDITION,
  SET_TOKEN_VISIBILITY,
  SHOW_ON_PLAYER_VIEW,
  START_COMBAT,
} from './ids.ts';
import { center, polygon, trail } from './hexgrid.ts';
import { bandLabel, bandOf, CONDITIONS, initiativeOf, MAX_COUNT, numbering, SCALE, visibleOrder } from './model.ts';
import type { Character, Combat, Grid, Hex, Instance, MapInfo, Props, Token } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

/** A colour of the design system by its token name; anything else is no colour. */
const tone = (color: string): string => (/^[a-z][a-z-]*$/.test(color) ? color : 'secondary');

/**
 * The Micro-UI, its regions empty: what never depends on the answers — the frame — is there from the first paint.
 * The carousel follows the body in the markup, so the first token a name finds is the map's; the stylesheet shows it above.
 */
export function skeleton(props: Props): string {
  const modes = `${props.fill ? ' bm--fill' : ''}${props.view === 'player' && !props.pilot ? ' bm--readonly' : ''}${props.view === 'prep' ? ' bm--prep' : ''}`;
  return (
    `<div class="bm${modes}" id="battlemap" data-reads="${GET_BATTLEMAP_INSTANCE}">` +
    '<div class="bm-toolbar" id="battlemap-toolbar"></div>' +
    '<div class="bm-body" id="battlemap-body">' +
    `<aside class="bm-side" id="battlemap-roster" aria-labelledby="battlemap-roster-title" data-reads="${LIST_CHARACTERS}"${props.view === 'prep' ? '' : ' hidden'}></aside>` +
    `<div class="bm-map" id="battlemap-map" data-reads="${GET_BATTLEMAP} ${GET_DIFFUSION}"></div>` +
    '<aside class="bm-side" id="token-inspector" aria-labelledby="inspector-title" aria-live="polite" hidden></aside>' +
    '</div>' +
    '<div class="bm-carousel" id="initiative-carousel" aria-label="Ordre d’initiative" hidden></div>' +
    '<p class="bm-status" id="battlemap-status" role="status" aria-live="polite" hidden></p>' +
    '<div id="battlemap-dialog"></div>' +
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
    group(
      (props.view === 'dm'
        ? instance.combat
          ? button({ id: 'bm-end-combat', label: 'Terminer le combat', variant: 'secondary', writes: END_COMBAT })
          : button({ id: 'bm-start-combat', label: 'Lancer le combat', variant: 'primary', writes: START_COMBAT })
        : '') +
        button({ id: 'bm-send', label: 'Envoyer aux joueurs', variant: 'secondary', writes: SHOW_ON_PLAYER_VIEW }) +
        button({ id: 'bm-reset', label: 'Réinitialiser la carte', variant: 'ghost', writes: RESET_BATTLEMAP }),
    )
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
      ? '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="Réduire le zoom">−</button>' +
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

interface Figure {
  view: Props['view'];
  selected: boolean;
  character?: Character;
}

/** Where a figure stands: on the map at its hex, or in the carousel at its rank. */
interface Place {
  id: string;
  style: string;
  cls: string[];
  /** What follows the name in the carousel: the participant's initiative. */
  extra: string;
  attrs: string;
}

/** The kit's Token for one token on the map, as the player or the DM reads it. */
export function tokenItem(t: Token, grid: Grid, p: Figure): string {
  const c = center(grid, t);
  return figure(t, p, { id: `token-${esc(t.id)}`, style: ` style="left:${c.x.toFixed(1)}px;top:${c.y.toFixed(1)}px"`, cls: [], extra: '', attrs: '' });
}

function figure(t: Token, p: Figure, place: Place): string {
  const player = p.view === 'player';
  const initials = p.character?.initials ?? t.initials;
  const color = p.character?.color || t.color;
  const cls = ['kit-token__item', ...place.cls, !t.visible && 'kit-token__item--hidden', t.dead && 'kit-token__item--dead', p.selected && 'bm-selected'].filter(Boolean).join(' ');
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
    `<div class="${cls}" id="${place.id}" data-token="${esc(t.id)}"${place.style}${place.attrs} title="${esc(t.name + (t.visible ? '' : ' — caché aux joueurs'))}">` +
    `<div class="kit-token__disc" style="--kit-token-bg: var(--color-${tone(color)})"><div class="kit-token__clip"><span class="kit-token__initials">${esc(initials)}</span></div>` +
    `${t.visible ? '' : eye}${t.dead ? '<span class="kit-token__strike"></span>' : ''}${badges}` +
    `${t.num === null ? '' : `<span class="kit-token__num">${t.num}</span>`}</div>` +
    `<div class="kit-token__name">${esc(t.name)}</div>${health}${place.extra}</div>`
  );
}

const group = (inner: string): string => `<div class="kit-button kit-button--bare kit-button--small">${inner}</div>`;

/** The initiative carousel: the round, the order by rank — a hidden token only for the DM, who sees it marked — and the DM's controls. */
export function carousel(combat: Combat, tokens: readonly Token[], p: { view: Props['view']; selected: string | null; characters: Record<string, Character>; pjInitiative: Record<string, number> }): string {
  const dm = p.view !== 'player';
  const items = visibleOrder(combat, tokens, p.view)
    .map(({ participant, token }) =>
      figure(token, { view: p.view, selected: token.id === p.selected, character: p.characters[token.char] }, {
        id: `carousel-${esc(token.id)}`,
        style: '',
        cls: token.id === combat.current ? ['kit-token__item--current'] : [],
        extra: `<span class="bm-init" title="Initiative">${participant.init}</span>`,
        attrs: dm ? ` draggable="true" data-participant="${esc(token.id)}"` : '',
      }),
    )
    .join('');
  const listed = new Set(combat.order.map((o) => o.id));
  const outside = tokens.filter((t) => !listed.has(t.id) && !t.dead);
  const chosen = p.selected !== null && listed.has(p.selected);
  const controls = dm
    ? '<div class="bm-carousel__controls" id="carousel-controls">' +
      group(
        button({ id: 'combat-next', label: 'Suivant ▸', variant: 'primary', writes: NEXT_TURN }) +
          button({ id: 'combat-earlier', label: '◂ Avancer', variant: 'ghost', writes: REORDER_INITIATIVE, disabled: !chosen }) +
          button({ id: 'combat-later', label: 'Reculer ▸', variant: 'ghost', writes: REORDER_INITIATIVE, disabled: !chosen }) +
          button({ id: 'combat-drop', label: 'Retirer du combat', variant: 'ghost', writes: REMOVE_PARTICIPANT, disabled: !chosen }),
      ) +
      (outside.length > 0
        ? `<span class="row"><select class="num-input bm-add" id="combat-add-select" aria-label="Participant à ajouter">${outside.map((t) => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join('')}</select>` +
          `<input type="number" class="num-input" id="combat-add-init" min="1" max="40" aria-label="Initiative du PJ ajouté" value="${p.pjInitiative[outside[0]?.char ?? ''] ?? 10}">` +
          `${group(button({ id: 'combat-add', label: 'Ajouter', variant: 'secondary', writes: ADD_PARTICIPANT }))}</span>`
        : '') +
      '</div>'
    : '';
  return `<div class="bm-carousel__round" id="combat-round"><span class="small muted">Round</span><b>${combat.round}</b></div><div class="bm-carousel__track" id="carousel-track"><div class="kit-token kit-token--row">${items}</div></div>${controls}`;
}

/** A kit Dialog over the Micro-UI: the veil, the window, a title with its close, the body and the footer's buttons. */
function dialog(p: { id: string; title: string; text?: string; body?: string; actions: Array<{ id: string; label: string; variant: 'primary' | 'ghost'; writes?: string }> }): string {
  return (
    `<div class="bm-modal" id="${p.id}"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(p.title)}">` +
    `<header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(p.title)}</h2><button class="kit-dialog__close" type="button" data-act="dialog-close" aria-label="Fermer">×</button></header>` +
    `<div class="kit-dialog__body">${p.text ? `<p class="kit-dialog__text">${esc(p.text)}</p>` : ''}${p.body ?? ''}</div>` +
    `<footer class="kit-dialog__footer">${p.actions.map((a) => `<button type="button" class="kit-dialog__btn kit-dialog__btn--${a.variant}" id="${a.id}"${attr('data-writes', a.writes ?? '')}>${esc(a.label)}</button>`).join('')}</footer>` +
    '</div></div></div></div>'
  );
}

/** One line of the start dialog: an NPC's d20 and what it makes, or the field where the DM enters a PC's initiative. */
export interface Starter {
  token: Token;
  roll: number;
  dexMod: number;
  entered: number | null;
}

const signed = (n: number): string => (n < 0 ? `− ${Math.abs(n)}` : `+ ${n}`);

/** « Lancer le combat » : the NPC roll d20 + DEX, the DM enters the PCs' initiative. */
export function startDialog(starters: readonly Starter[]): string {
  const rows = starters
    .map(
      (s) =>
        `<li class="kit-dialog__row"><span>${esc(s.token.name)}${s.token.visible ? '' : ' <em class="muted">(caché)</em>'}</span>` +
        (s.token.kind === 'pnj'
          ? `<strong>d20 (${s.roll}) ${signed(s.dexMod)} = ${initiativeOf(s.roll, s.dexMod)}</strong>`
          : `<input type="number" class="num-input" data-init="${esc(s.token.id)}" min="1" max="40" value="${s.entered ?? ''}" aria-label="${esc(`Initiative de ${s.token.name}`)}">`) +
        '</li>',
    )
    .join('');
  return dialog({
    id: 'combat-start-dialog',
    title: 'Lancer le combat',
    body: `<p class="kit-dialog__text">Les PNJ tirent d20 + modificateur de DEX ; les égalités se départagent par la DEX, puis au hasard. Saisissez l’initiative des PJ.</p><ul class="kit-dialog__list" id="combat-setup">${rows}</ul><p class="bm-status is-error" id="combat-start-error" role="alert" hidden></p>`,
    actions: [
      { id: 'combat-start-cancel', label: 'Annuler', variant: 'ghost' },
      { id: 'combat-start-submit', label: 'Commencer le combat', variant: 'primary', writes: START_COMBAT },
    ],
  });
}

/** « Réinitialiser la carte » : the Dialog where the DM confirms. */
export const resetDialog = (): string =>
  dialog({
    id: 'battlemap-reset-dialog',
    title: 'Réinitialiser la carte',
    text: 'Les tokens, le combat et l’initiative de cette instance sont effacés ; la carte repart comme à son ouverture.',
    actions: [
      { id: 'battlemap-reset-cancel', label: 'Annuler', variant: 'ghost' },
      { id: 'battlemap-reset-submit', label: 'Réinitialiser', variant: 'primary', writes: RESET_BATTLEMAP },
    ],
  });

/** The path a carried token would follow, over the overlay: every hex a step, the last one the end. */
export function pathMarkup(grid: Grid, hexes: Hex[]): string {
  return hexes.map((h, i) => `<polygon class="${i === hexes.length - 1 ? 'hm-end' : 'hm-step'}" points="${polygon(grid, h)}"></polygon>`).join('') + `<polyline class="hm-line" points="${trail(grid, hexes)}"></polyline>`;
}

/** The token's menu: its name, whether the players see it, its conditions, its removal. */
export function inspector(t: Token, siblings: readonly Token[], character: Character | undefined, initiative: number | null = null): string {
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
    `<p class="small muted">${esc(character?.name ?? '')}${initiative === null ? '' : ` · initiative ${initiative}`}</p>` +
    chips +
    hitPoints(t) +
    `<div class="kit-input is-wide"><label>Nom affiché<input type="text" id="inspector-name" value="${esc(t.name)}" data-writes="${RENAME_TOKEN}"></label></div>` +
    `<div class="kit-toggle kit-toggle--bare"><label><button type="button" role="switch" id="inspector-visible" aria-checked="${t.visible}" data-writes="${SET_TOKEN_VISIBILITY}"><i></i></button><span>${t.visible ? 'Visible des joueurs' : 'Caché aux joueurs'}</span></label></div>` +
    `<div id="inspector-states" class="stack"><h3>États</h3><div class="row">${conditions}</div></div>` +
    (t.dead ? '<p class="bm-dead" id="inspector-dead">Mort — grisé.</p>' : '') +
    `<div class="kit-button kit-button--bare kit-button--small">${button({ id: 'inspector-remove', label: t.dead ? 'Retirer le corps' : 'Retirer de la carte', variant: 'danger', writes: REMOVE_TOKEN })}</div>`
  );
}

/** The NPC's current and maximum HP with the field to change them; a PC's are not tracked (pj-hp-not-tracked). */
function hitPoints(t: Token): string {
  if (t.kind !== 'pnj' || t.hp === null || t.hpMax === null) return '<p class="small muted" id="inspector-pj">Les PV des PJ ne sont pas suivis : les joueurs les gèrent sur leur fiche.</p>';
  return (
    `<div class="bm-hp" id="inspector-hp"><div class="bm-hp__value"><b>${t.hp}</b> / ${t.hpMax} PV</div><span class="small muted">Vu par les joueurs : ${esc(bandLabel(bandOf(t.hp, t.hpMax)))}</span>` +
    '<div class="row"><input type="number" class="num-input" id="inspector-hp-amount" min="1" max="999" value="7" aria-label="Points de vie">' +
    `<div class="kit-button kit-button--bare kit-button--small">${button({ id: 'inspector-damage', label: '− Dégâts', variant: 'danger', writes: CHANGE_HIT_POINTS })}${button({ id: 'inspector-heal', label: '+ Soins', variant: 'secondary', writes: CHANGE_HIT_POINTS })}</div></div></div>`
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
