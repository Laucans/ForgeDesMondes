// The list's markup, from mock/components/CampaignList.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { CREATE_CAMPAIGN, EXPORT_CAMPAIGN, IMPORT_CAMPAIGN, LIST_CAMPAIGNS } from './ids.ts';
import { dayText, lastPlayedText, sessionsText } from './model.ts';
import type { CampaignCard } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const reads = `data-reads="${LIST_CAMPAIGNS}"`;

/** The list, its cards empty: the two buttons of the header are there from the first paint. */
export function skeleton(): string {
  return (
    '<section class="mf-campaigns" id="campaign-list" aria-labelledby="campaign-list-title">' +
    '<div class="panel__head"><h2 id="campaign-list-title">Campagnes et oneshots</h2>' +
    '<div id="campaign-list-actions"><div class="kit-button kit-button--bare">' +
    `<button type="button" class="primary" id="campaign-new" data-writes="${CREATE_CAMPAIGN}">Nouvelle campagne</button>` +
    `<button type="button" class="secondary" id="campaign-import" data-writes="${IMPORT_CAMPAIGN}">Importer une archive</button>` +
    '</div></div></div>' +
    '<p class="small muted" id="campaign-status" aria-live="polite"></p>' +
    `<div class="mf-campaigns__grid" id="campaign-cards" ${reads}></div>` +
    '<div id="campaign-dialogs"></div>' +
    '</section>'
  );
}

/** The cover: a gradient drawn from the tokens, the campaign's glyph. */
function cover(campaign: CampaignCard): string {
  const art = /^[a-z]+$/.test(campaign.art) ? campaign.art : 'ember';
  return (
    `<div class="art art--${art}" role="img" aria-label="${esc(`Couverture : ${campaign.name}`)}">` +
    (campaign.glyph === '' ? '' : `<span class="art__glyph">${esc(campaign.glyph)}</span>`) +
    '</div>'
  );
}

/** The kit's Card: cover, name, pitch, the kind, the sessions, when it was last played and the next one. */
export function card(campaign: CampaignCard): string {
  const sessions = sessionsText(campaign.sessions);
  const meta =
    '<div class="mf-campaigns__meta">' +
    `<div class="kit-badge"><span class="${campaign.type === 'oneshot' ? 'accent' : 'primary'}">${campaign.type === 'oneshot' ? 'Oneshot' : 'Campagne'}</span></div>` +
    (sessions === '' ? '' : `<span>${sessions}</span>`) +
    `<span>${sessions === '' ? '' : '· '}${esc(lastPlayedText(campaign.lastPlayed))}</span></div>` +
    (campaign.next === null ? '' : `<p class="small">Prochaine séance : <b>${esc(dayText(campaign.next))}</b></p>`);
  return (
    `<div id="campaign-${esc(campaign.id)}" ${reads}><article class="kit-card">` +
    `<div class="media">${cover(campaign)}</div>` +
    `<div class="body"><h3>${esc(campaign.name)}</h3>${campaign.pitch === '' ? '' : `<p>${esc(campaign.pitch)}</p>`}${meta}</div>` +
    '<div class="actions">' +
    `<button type="button" data-href="campagne.html?id=${encodeURIComponent(campaign.id)}">Ouvrir</button>` +
    `<button type="button" class="secondary" data-action="export" data-campaign="${esc(campaign.id)}" data-writes="${EXPORT_CAMPAIGN}">Exporter</button>` +
    '</div></article></div>'
  );
}

export const cards = (campaigns: CampaignCard[]): string => campaigns.map(card).join('');

export const empty = (): string => '<p class="muted" id="campaign-empty">Aucune campagne pour l’instant : créez-en une ou importez une archive.</p>';

/** The kit's Dialog (mock/kit/Dialog.html), opened as a modal over the page. */
function dialog(id: string, title: string, body: string, submit: { id: string; label: string; writes: string; disabled?: boolean }): string {
  return (
    `<div class="mf-modal" id="${id}"><div class="kit-dialog"><div class="kit-dialog__veil">` +
    `<div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(title)}">` +
    `<header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(title)}</h2>` +
    '<button class="kit-dialog__close" type="button" aria-label="Fermer" data-close>×</button></header>' +
    `<div class="kit-dialog__body">${body}</div>` +
    '<footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" data-close>Annuler</button>' +
    `<button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="${submit.id}" data-writes="${submit.writes}"${submit.disabled ? ' disabled' : ''}>${esc(submit.label)}</button></footer>` +
    '</div></div></div></div>'
  );
}

/** The kit's Input (mock/kit/Input.html). */
const input = (id: string, label: string, placeholder: string): string =>
  `<div class="kit-input is-wide"><label>${esc(label)}<input type="text" id="${id}" placeholder="${esc(placeholder)}"></label></div>`;

const UPLOAD_ICON =
  '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';

/** The kit's FileDrop (mock/kit/FileDrop.html), for the archive. */
function fileDrop(): string {
  return (
    '<div class="kit-filedrop is-wide" id="campaign-import-field"><label class="kit-filedrop__zone" tabindex="0" id="campaign-import-drop">' +
    UPLOAD_ICON +
    '<span class="kit-filedrop__title">Déposez l’archive ou <b>parcourez</b></span><span class="kit-filedrop__hint">Fichier .json écrit par un export</span>' +
    '<span class="kit-filedrop__types"><span class="kit-filedrop__tag">JSON</span></span>' +
    '<input class="kit-filedrop__input" type="file" id="campaign-import-file" accept=".json,application/json"></label>' +
    '<div class="kit-filedrop__error">Ce fichier n’est pas une archive.</div>' +
    '<div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info"><div class="kit-filedrop__name"></div>' +
    '<div class="kit-filedrop__row"><span class="kit-filedrop__size"></span><span class="kit-filedrop__pct">Prêt</span></div>' +
    '<div class="kit-filedrop__bar"><div class="kit-filedrop__fill"></div></div></div></div></div>'
  );
}

/** « Nouvelle campagne »: a name, a kind, and a pitch the card will show. */
export function createDialog(): string {
  return dialog(
    'campaign-create-dialog',
    'Nouvelle campagne',
    input('campaign-create-name', 'Nom', 'Les Cendres de Valombre') +
      '<div class="kit-radio"><fieldset id="campaign-create-type"><legend>Type</legend>' +
      '<label><input type="radio" name="campaign-type" value="campagne" checked><span>Campagne</span><small>plusieurs séances</small></label>' +
      '<label><input type="radio" name="campaign-type" value="oneshot"><span>Oneshot</span><small>une seule séance</small></label></fieldset></div>' +
      input('campaign-create-pitch', 'Pitch (facultatif)', 'Un bourg couvert de cendres…') +
      '<p class="small is-error" id="campaign-create-error" aria-live="polite"></p>',
    { id: 'campaign-create-submit', label: 'Créer', writes: CREATE_CAMPAIGN },
  );
}

/** « Importer une archive »: the archive an export wrote; an import always makes a new campaign. */
export function importDialog(): string {
  return dialog(
    'campaign-import-dialog',
    'Importer une archive',
    '<p class="kit-dialog__text">Une archive exportée depuis ForgeDesMondes, sur cette machine ou une autre. L’import crée toujours une nouvelle campagne : l’originale n’est jamais modifiée.</p>' +
      fileDrop() +
      '<p class="small is-error" id="campaign-import-error" aria-live="polite"></p>',
    { id: 'campaign-import-submit', label: 'Importer', writes: IMPORT_CAMPAIGN, disabled: true },
  );
}
