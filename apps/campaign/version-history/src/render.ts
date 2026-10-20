// The panel's markup, from mock/components/VersionHistory.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { LIST_ENTITY_VERSIONS, RESTORE_ENTITY_VERSION } from './ids.ts';
import { hasPrevious, originBadge, sourceLabel } from './model.ts';
import type { History, Version } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const reads = `data-reads="${LIST_ENTITY_VERSIONS}"`;

const when = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString('fr-FR', { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' });
};

/** The panel, its regions empty: the trigger and the title come once the history is read. */
export function skeleton(): string {
  return (
    `<div class="mf-versions" id="version-history" ${reads}>` +
    '<div class="panel__head" id="versions-head" hidden><h2 id="versions-title"></h2></div>' +
    `<div id="versions-trigger" ${reads}></div>` +
    '<p class="small muted" id="versions-note" hidden></p>' +
    '</div>'
  );
}

/** The kit's Button, small: the history's door — « Mis à jour » when the DM has not seen the last changes. */
export function trigger(unseen: number): string {
  const button =
    unseen > 0
      ? `<button type="button" class="primary" id="versions-updated" title="L’IA a modifié la fiche : voir le diff et l’historique">Mis à jour${unseen > 1 ? ` (${unseen})` : ''}</button>`
      : '<button type="button" class="ghost" id="versions-open">Historique des versions</button>';
  return `<div class="kit-button kit-button--bare kit-button--small">${button}</div>`;
}

/** The kit's DiffView of one version, field by field; `pick` offers the kit's per-change undo, which a version with nothing before it cannot have. */
export function diff(v: Version, pick: boolean): string {
  const rows = v.changes
    .map(
      (c, i) =>
        `<li class="kit-diffview__row" data-index="${i}"><div class="kit-diffview__field">${esc(c.field)}</div><div class="kit-diffview__values">` +
        `<del class="kit-diffview__old">${esc(c.old || '—')}</del><ins class="kit-diffview__new">${esc(c.new || '—')}</ins></div>` +
        (pick ? '<label class="kit-diffview__action"><input type="checkbox" class="kit-diffview__check"><span>Annuler ce changement</span></label>' : '') +
        '</li>',
    )
    .join('');
  return (
    `<div class="kit-diffview"><header class="kit-diffview__head"><h2 class="kit-diffview__title">${esc(`Version ${v.v} — ${sourceLabel(v)}`)}</h2>` +
    `<span class="kit-diffview__meta">${esc(`${when(v.date)} · ${v.changes.length} champ(s)`)}</span></header><ul class="kit-diffview__list">${rows}</ul></div>`
  );
}

function item(v: Version, isNewest: boolean, isUnseen: boolean): string {
  const badge = originBadge(v.origin);
  const badges = [`<span${badge.tone === null ? '' : ` class="${badge.tone}"`}>${esc(badge.label)}</span>`];
  if (isUnseen) badges.push('<span class="accent">Non vue</span>');
  const restore = isNewest
    ? ''
    : `<button type="button" class="secondary" data-action="restore:${v.v}" data-writes="${RESTORE_ENTITY_VERSION}">Revenir à cette version</button>`;
  return (
    `<li class="mf-versions__item${isUnseen ? ' is-unseen' : ''}" id="version-${v.v}">` +
    `<div class="panel__head"><b>v${v.v}</b><div class="kit-badge">${badges.join('')}</div></div>` +
    `<span>${esc(sourceLabel(v))}</span><span class="small muted">${esc(`${when(v.date)} · ${v.changes.map((c) => c.field).join(', ')}`)}</span>` +
    `<div class="kit-button kit-button--bare kit-button--small"><button type="button" class="ghost" data-action="show:${v.v}">Voir le diff</button>${restore}</div></li>`
  );
}

/** What the modal holds: the diff of `target`, then every version, newest first. */
export function body(history: History, target: Version | undefined, unseen: ReadonlySet<number>): string {
  return (
    `<div id="versions-diff" ${reads}>${target ? diff(target, hasPrevious(history, target)) : '<p>Aucune version.</p>'}</div>` +
    '<h3>Historique</h3>' +
    `<ul class="mf-versions__list" id="versions-list" ${reads}>${history.versions.map((v, i) => item(v, i === 0, unseen.has(v.v))).join('')}</ul>` +
    '<p class="mf-versions__status" id="versions-status" aria-live="polite" hidden></p>'
  );
}

/** The kit's Dialog, opened over the page. */
export function dialog(name: string, inner: string): string {
  return (
    '<div class="mf-modal mf-modal--wide" id="versions-dialog"><div class="kit-dialog"><div class="kit-dialog__veil">' +
    `<div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(`Versions de « ${name} »`)}">` +
    `<header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(`Versions de « ${name} »`)}</h2>` +
    '<button class="kit-dialog__close" type="button" aria-label="Fermer">×</button></header>' +
    `<div class="kit-dialog__body" id="versions-body">${inner}</div>` +
    '<footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="versions-close">Fermer</button>' +
    `<button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="versions-revert" data-action="revert" data-writes="${RESTORE_ENTITY_VERSION}">Annuler les changements cochés</button></footer>` +
    '</div></div></div></div>'
  );
}
