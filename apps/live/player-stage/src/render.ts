// The stage's markup, from mock/components/PlayerStage.html: the same ids, the
// same classes. Every part names the unit it shows (`data-reads`) or triggers
// (`data-writes`) by identifier. Only what a player may see is drawn: a title,
// an image, a battlemap slot — the tokens over it are the Battlemap's, composed
// in view 'player'.

import { BATTLEMAP, GET_CAMPAIGN, GET_DIFFUSION, GET_LIVE_SESSION, LIST_MEDIA, MOVE_WINDOW, RESIZE_WINDOW } from './ids.ts';
import type { Campaign, Illustration, Media } from './types.ts';
import { illustrationOf } from './model.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** An image as the library holds it: its art and glyph, named for a screen reader. */
export function art(image: { art: string; glyph: string; title: string }): string {
  const name = /^[a-z]+$/.test(image.art) ? image.art : 'ember';
  return `<div class="art art--${name}" role="img" aria-label="${esc(image.title)}">${image.glyph === '' ? '' : `<span class="art__glyph">${esc(image.glyph)}</span>`}</div>`;
}

/** The stage, its regions empty: the corner is on every stage, the bar and the badge only in the DM's tab. */
export function skeleton(pilot: boolean): string {
  const bar = pilot ? `<figcaption class="pv-window__bar" id="player-window-bar" data-writes="${MOVE_WINDOW}"></figcaption>` : '';
  const corner = `<span class="pv-window__resize" id="player-window-resize" data-writes="${RESIZE_WINDOW}" aria-hidden="true"></span>`;
  const badge = pilot ? '<p class="pv-pilot" id="player-pilot-badge">Vue pilotée par le MJ · déplacez les tokens et la fenêtre, zoomez la carte</p>' : '';
  return (
    `<div class="pv${pilot ? ' pv--pilot' : ''}" id="player-stage" data-reads="${GET_LIVE_SESSION}">` +
    `<div class="pv-base" id="player-base" data-reads="${GET_DIFFUSION}"></div>` +
    `<figure class="pv-window" id="player-window" hidden>${bar}<div class="pv-window__art" id="player-window-art" data-reads="${LIST_MEDIA}"></div>${corner}</figure>` +
    badge +
    '<p class="pv-status" id="player-status" role="status" hidden></p>' +
    '</div>'
  );
}

/** The campaign's cover and title; its session when the campaign says which one comes next. The title gives way while a window is shown. */
export function cover(campaign: Campaign | null, media: Media, titled: boolean): string {
  const image = illustrationOf(media, campaign?.cover || media.cover);
  const picture = image ?? { art: campaign?.art ?? '', glyph: campaign?.glyph ?? '', title: 'Couverture de la campagne' };
  const title = campaign === null || !titled ? '' : `<h1 id="player-cover-title">${esc(campaign.name)}</h1>${campaign.session === null ? '' : `<p>Séance ${campaign.session.order} — ${esc(campaign.session.title)}</p>`}`;
  return `<div class="pv-cover" id="player-cover" data-reads="${GET_CAMPAIGN}">${art({ ...picture, title: 'Couverture de la campagne' })}<div class="pv-cover__title">${title}</div></div>`;
}

/** An illustration sent as the scene, whole, at its own ratio. */
export function scene(image: Illustration): string {
  return `<div class="pv-scene" id="player-scene"><div class="pv-scene__art" data-reads="${LIST_MEDIA}" style="aspect-ratio: ${image.ratio}">${art(image)}</div></div>`;
}

/** Where the battlemap goes: the screen composes the Battlemap in view 'player' here. */
export function mapSlot(instance: string, pilot: boolean): string {
  const props = JSON.stringify({ instance, view: 'player', pilot, fill: true });
  return `<div class="pv-map" id="player-map" data-component="${BATTLEMAP}" data-props="${esc(props)}"></div>`;
}
