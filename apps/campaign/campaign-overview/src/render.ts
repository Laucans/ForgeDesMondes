// The panel's markup, from mock/components/CampaignOverview.html and the
// kit's own (mock/kit/<Name>.html): the same ids, the same classes. Every
// part names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { EXPORT_CAMPAIGN, GET_CAMPAIGN } from './ids.ts';
import { countsOf, nextSessionDay } from './model.ts';
import type { Campaign } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const reads = `data-reads="${GET_CAMPAIGN}"`;

/** The panel, its regions empty: the export button is there from the first paint. */
export function skeleton(): string {
  return (
    `<section class="mf-overview" id="campaign-overview" aria-labelledby="overview-title" ${reads}>` +
    `<div class="mf-overview__cover" id="overview-cover" ${reads}></div>` +
    '<div class="mf-overview__body">' +
    `<div class="row" id="overview-type" ${reads}></div>` +
    `<h1 id="overview-title" ${reads}></h1>` +
    `<p class="muted" id="overview-pitch" ${reads}></p>` +
    `<ul class="mf-overview__counts" id="overview-counts" ${reads}></ul>` +
    '<div id="overview-actions"></div>' +
    `<p class="small muted" id="overview-export-status" ${reads} aria-live="polite"></p>` +
    '</div></section>'
  );
}

/** The cover: a gradient drawn from the tokens, the campaign's glyph. */
export function cover(campaign: Campaign): string {
  const art = /^[a-z]+$/.test(campaign.art) ? campaign.art : 'ember';
  return (
    `<div class="art art--${art}" role="img" aria-label="${esc(`Couverture : ${campaign.name}`)}">` +
    (campaign.glyph === '' ? '' : `<span class="art__glyph">${esc(campaign.glyph)}</span>`) +
    '<span class="art__caption">Couverture · affichée sur la player view</span></div>'
  );
}

/** The kit's Badge: the kind of the campaign, and the next session when the answer has one. */
export function badges(campaign: Campaign): string {
  const next = campaign.nextSession;
  return (
    '<div class="kit-badge">' +
    `<span class="primary">${campaign.type === 'oneshot' ? 'Oneshot' : 'Campagne'}</span>` +
    (next === null ? '' : `<span class="accent">${esc(`Prochaine : ${nextSessionDay(next.date)}`)}</span>`) +
    '</div>'
  );
}

export function counts(campaign: Campaign): string {
  return countsOf(campaign)
    .map(({ value, label }) => `<li><b>${value}</b>${esc(label)}</li>`)
    .join('');
}

/** The kit's Button: launch and prepare the next session when there is one, and always the export. */
export function actions(campaign: Campaign): string {
  const next = campaign.nextSession;
  return (
    '<div class="kit-button kit-button--bare">' +
    (next === null
      ? ''
      : `<button type="button" class="primary" id="overview-launch" data-href="dm-view.html">${esc(`Lancer la séance ${next.order} en direct`)}</button>` +
        `<button type="button" class="secondary" id="overview-prepare" data-href="preparation-seance.html">${esc(`Préparer la séance ${next.order}`)}</button>`) +
    `<button type="button" class="ghost" id="overview-export" data-writes="${EXPORT_CAMPAIGN}">Exporter l’archive</button>` +
    '</div>'
  );
}
