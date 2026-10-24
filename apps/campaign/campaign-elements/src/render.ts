// The panel's markup, from mock/components/CampaignElements.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part that
// shows what a Capability answers names it (`data-reads`) by identifier; the
// Micro-UI triggers no DataCapability, so no part carries `data-writes`.

import { LIST_CHARACTERS, LIST_EVENTS, LIST_MEDIA, LIST_PLACES, LIST_PLAYERS, LIST_SOUNDS } from './ids.ts';
import { GROUPS, level, placeGlyph, synced } from './model.ts';
import type { Character, Group, Media, Named, Player, RailEntry, Sound } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const reads = (...ids: string[]): string => `data-reads="${ids.join(' ')}"`;

/** The mock-up's illustration helper: a tinted card with a glyph. */
function art(name: string, glyph: string): string {
  const tone = /^[a-z]+$/.test(name) ? name : 'ember';
  return `<div class="art art--${tone}" role="img">${glyph === '' ? '' : `<span class="art__glyph">${esc(glyph)}</span>`}</div>`;
}

/** The sheet of an element, where the tabs' links go (the mock-up's page protocol). */
export const sheetHref = (id: string): string => `fiche-entite.html?id=${encodeURIComponent(id)}`;

/** Where « Nouveau… » leads from the tabs, by the kind of element it starts. */
export const NEW_KINDS: Readonly<Record<string, string>> = { pnj: 'pnj', place: 'lieu', event: 'evenement' };
const NEW_LABELS: ReadonlyArray<readonly [string, string, string]> = [
  ['pnj', 'Nouveau PNJ', 'secondary'],
  ['place', 'Nouveau lieu', 'ghost'],
  ['event', 'Nouvel événement', 'ghost'],
];

const smallButtons = (buttons: string): string => `<div class="kit-button kit-button--bare kit-button--small">${buttons}</div>`;

/** The kit's Tabs: one tab and one panel per type of element, the counts come with the answers. */
export const TABS: ReadonlyArray<readonly [string, string, string]> = [
  ['players', 'Joueurs', LIST_PLAYERS],
  ['characters', 'Personnages', LIST_CHARACTERS],
  ['places', 'Lieux', LIST_PLACES],
  ['events', 'Événements', LIST_EVENTS],
  ['media', 'Médias', LIST_MEDIA],
  ['sounds', 'Sons', LIST_SOUNDS],
];
export const FIRST_TAB = 1;

/** Tabs mode, its panels still empty. */
export function tabsSkeleton(): string {
  const newButtons = NEW_LABELS.map(([kind, label, variant]) => `<button type="button" class="${variant}" id="elements-new-${kind}" data-href="fiche-entite.html?new=${NEW_KINDS[kind] ?? ''}">${esc(label)}</button>`).join('');
  const tabs = TABS.map(
    ([key, label], i) => `<button type="button" role="tab" id="elements-tabs-tab-${i}" aria-controls="elements-tabs-panel-${i}" aria-selected="${i === FIRST_TAB}" data-tab="${key}">${esc(label)}</button>`,
  ).join('');
  const panels = TABS.map(
    ([key, , read], i) => `<div role="tabpanel" id="elements-tabs-panel-${i}" aria-labelledby="elements-tabs-tab-${i}" data-panel="${key}" ${reads(read)}${i === FIRST_TAB ? '' : ' hidden'}><p class="small muted">Chargement…</p></div>`,
  ).join('');
  return (
    '<section class="mf-elements" id="campaign-elements" aria-labelledby="campaign-elements-title">' +
    `<div class="panel__head"><h2 id="campaign-elements-title">Éléments de la campagne</h2>${smallButtons(newButtons)}</div>` +
    `<div id="elements-tabs-host" ${reads(LIST_PLAYERS, LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS, LIST_MEDIA, LIST_SOUNDS)}>` +
    `<div class="kit-tabs"><div role="tablist" aria-label="Éléments par type">${tabs}</div>${panels}</div></div></section>`
  );
}

/** « Joueurs (4) »: the tab once its answer is in. */
export const tabLabel = (label: string, count: number): string => `${label} (${count})`;

export const unreadable = (what: string): string => `<p class="small muted is-error" role="alert">Impossible de lire ${esc(what)}.</p>`;

function entry(href: string, id: string, thumb: string, title: string, sub: string): string {
  return `<li><a class="mf-elements__entry" id="${esc(id)}" href="${esc(href)}"><span class="mf-elements__thumb">${thumb}</span><span><b>${esc(title)}</b><small>${esc(sub)}</small></span></a></li>`;
}

const grid = (items: string[]): string => `<ul class="mf-elements__grid">${items.join('')}</ul>`;

const when = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleDateString('fr-FR');
};

/** The kit's Table: each player with the characters they play, their D&D Beyond sync and level. */
export function playersPanel(players: Player[], characters: Character[], charactersUnreadable = false): string {
  const rows = players
    .map((player) => {
      const played = characters.filter((c) => c.kind === 'pj' && c.player === player.id);
      const cells = [
        esc(player.name),
        played.map((c) => `<a href="${esc(sheetHref(c.id))}">${esc(c.name)}</a>`).join(', '),
        played.map((c) => esc(synced(c, when))).join(', '),
        played.map((c) => esc(level(c.type))).join(', '),
      ];
      return `<tr id="player-${esc(player.id)}">${cells.map((cell, i) => `<td${i === 3 ? ' class="num"' : ''}>${cell}</td>`).join('')}</tr>`;
    })
    .join('');
  const note = charactersUnreadable ? unreadable('les personnages') : '';
  return `${note}<table class="kit-table" id="elements-players-table"><thead><tr><th>Joueur</th><th>Personnage</th><th>D&amp;D Beyond</th><th class="num">Niveau</th></tr></thead><tbody>${rows}</tbody></table>`;
}

const characterEntry = (c: Character): string =>
  entry(sheetHref(c.id), `elements-char-${c.id}`, art(c.art || 'night', c.initials), c.name, `${c.kind === 'pj' ? 'PJ' : 'PNJ'} · ${c.aliases.join(', ') || c.type}`);

/** The PJ, then the PNJ under their own title. */
export function charactersPanel(characters: Character[]): string {
  return `${grid(characters.filter((c) => c.kind === 'pj').map(characterEntry))}<h3 class="mf-elements__links">PNJ</h3>${grid(characters.filter((c) => c.kind === 'pnj').map(characterEntry))}`;
}

export const placesPanel = (places: Named[]): string =>
  grid(places.map((p) => entry(sheetHref(p.id), `elements-place-${p.id}`, art(p.art || 'night', placeGlyph(p.name)), p.name, p.aliases.join(', '))));

export const eventsPanel = (events: Named[]): string =>
  grid(events.map((v) => entry(sheetHref(v.id), `elements-event-${v.id}`, art('blood', '!'), v.name, v.aliases.join(', '))));

const link = (label: string, href: string): string => smallButtons(`<button type="button" class="secondary" data-href="${esc(href)}">${esc(label)}</button>`);

export const mediaPanel = (media: Media[]): string =>
  grid(media.map((m) => entry('medias.html', `elements-media-${m.id}`, art(m.art, m.glyph), m.title, `${m.kind === 'battlemap' ? 'Battlemap' : 'Illustration'} · ${m.size}`))) +
  `<div class="mf-elements__links">${link('Gérer les médias et les grilles', 'medias.html')}</div>`;

export const soundsPanel = (sounds: Sound[]): string =>
  grid(sounds.map((s) => `<li class="item" id="elements-sound-${esc(s.id)}"><span>♪ ${esc(s.name)}</span><small class="muted">${esc(s.tags.join(' · '))}</small></li>`)) +
  `<div class="mf-elements__links">${link('Ouvrir la bibliothèque de sons', 'soundbox.html')}</div>`;

/** Rail mode: a button that opens the drawer, the veil behind it, and the panel itself. */
export function railSkeleton(): string {
  const newButtons = NEW_LABELS.map(([kind, label, variant]) => `<button type="button" class="${variant}" id="rail-new-${kind}">${esc(label)}</button>`).join('');
  return (
    '<div class="mf-rail" id="elements-rail" data-collapsed="false" data-open="false">' +
    '<button type="button" class="chip mf-rail__open" id="rail-open" aria-controls="rail-panel" aria-expanded="false">☰ Éléments</button>' +
    '<div class="mf-rail__veil" id="rail-veil" hidden></div>' +
    '<nav class="mf-rail__panel" id="rail-panel" aria-label="Éléments de la campagne">' +
    '<div class="mf-rail__bar"><button type="button" class="icon-btn" id="rail-toggle" aria-controls="rail-body" aria-expanded="true" title="Replier la liste">«</button><h2 id="rail-title">Éléments</h2></div>' +
    '<span class="mf-rail__strip" id="rail-strip" aria-hidden="true">Éléments</span>' +
    '<div class="mf-rail__body" id="rail-body">' +
    '<div id="rail-search"><div class="kit-input"><label>Rechercher<input type="search" id="rail-search-input" placeholder="Nom ou alias" autocomplete="off"></label></div></div>' +
    '<p class="small muted" id="rail-count" aria-live="polite"></p>' +
    `<div class="mf-rail__groups" id="rail-groups" ${reads(LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS)}></div>` +
    `<div class="mf-rail__new" id="rail-new">${smallButtons(newButtons)}</div>` +
    '</div></nav></div>'
  );
}

/** The Capability each section of the rail shows. */
const GROUP_READS: Readonly<Record<Group, string>> = { pj: LIST_CHARACTERS, pnj: LIST_CHARACTERS, place: LIST_PLACES, event: LIST_EVENTS };

function railEntry(e: RailEntry, current: string): string {
  const here = e.id === current;
  return (
    `<li><a class="mf-rail__entry${here ? ' is-current' : ''}" id="rail-entry-${esc(e.id)}" href="${esc(sheetHref(e.id))}" data-entity="${esc(e.id)}"${here ? ' aria-current="true"' : ''}>` +
    `<span class="mf-rail__thumb">${art(e.art, e.glyph)}</span><span class="mf-rail__text"><b>${esc(e.name)}</b><small>${esc(e.sub)}</small></span></a></li>`
  );
}

/** The sections of the rail that have a line to show; a folded one keeps its title and hides its list. */
export function railGroups(found: RailEntry[], folded: ReadonlySet<Group>, current: string): string {
  return GROUPS.map(([group, label]) => {
    const rows = found.filter((e) => e.group === group);
    if (rows.length === 0) return '';
    const closed = folded.has(group);
    return (
      `<section class="mf-rail__group" id="rail-group-${group}" aria-label="${label}" ${reads(GROUP_READS[group])}>` +
      `<h3><button type="button" class="mf-rail__group-toggle" id="rail-group-toggle-${group}" data-group="${group}" aria-expanded="${!closed}" aria-controls="rail-list-${group}">` +
      `<span class="mf-rail__caret" aria-hidden="true">${closed ? '▸' : '▾'}</span>${label}<small>${rows.length}</small></button></h3>` +
      `<ul class="mf-rail__list" id="rail-list-${group}"${closed ? ' hidden' : ''}>${rows.map((e) => railEntry(e, current)).join('')}</ul></section>`
    );
  }).join('');
}
