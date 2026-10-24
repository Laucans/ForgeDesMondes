// The Micro-UI's markup, from mock/components/ToCreateList.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part
// names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { CREATE_FROM_BRIEF, GET_ENTITY, LIST_ENTITIES_TO_CREATE } from './ids.ts';
import { signed } from './model.ts';
import type { Item, Outcome, Proposal } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const MIC =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
const PAUSE =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';

/** The mic's states, in words. */
export const MIC_TEXTS = { idle: 'Prêt à dicter', recording: 'Enregistrement', paused: 'En pause' } as const;
export type MicState = keyof typeof MIC_TEXTS;

/** « 01:05 ». */
export const clock = (seconds: number): string => `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;

/** What the screen holds about one entry besides the entry itself. */
export interface View {
  refining: boolean;
  /** The refinement being written or dictated. */
  typed: string;
  mic: MicState;
  seconds: number;
  outcome: Outcome | null;
  /** What the AI proposed, once the entry is created from this screen. */
  proposal: Proposal | null;
}

export const freshView = (): View => ({ refining: false, typed: '', mic: 'idle', seconds: 0, outcome: null, proposal: null });

const TYPES: Record<string, string> = { pnj: 'PNJ', lieu: 'Lieu', 'événement': 'Événement' };

interface Action {
  action: string;
  label: string;
  variant: string;
  writes?: string;
}

/** The kit's Button, small: each actuator names the DataCapability it triggers when it triggers one. */
function buttons(actions: readonly Action[], locked: boolean): string {
  return (
    '<div class="kit-button kit-button--bare kit-button--small">' +
    actions
      .map((a) => `<button type="button" class="${a.variant}" data-action="${a.action}"${a.writes === undefined ? '' : ` data-writes="${a.writes}"`}${locked ? ' disabled' : ''}>${esc(a.label)}</button>`)
      .join('') +
    '</div>'
  );
}

/** The kit's StatBlock: the vitals are a list of their own, so the proposal keeps the one description list. */
function statblock(name: string, sheet: NonNullable<Proposal['sheet']>): string {
  const vitals = [
    sheet.ca === null ? '' : `<li class="kit-statblock__vital"><strong>CA</strong> <span>${sheet.ca}</span></li>`,
    sheet.pvMax === null ? '' : `<li class="kit-statblock__vital"><strong>PV</strong> <span>${sheet.pvMax}</span></li>`,
  ].join('');
  const abilities = sheet.abilities
    .map(
      (a) =>
        `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${a.key}</span><span class="kit-statblock__score">${a.score ?? '—'}</span>` +
        `${a.modifier === null ? '' : `<span class="kit-statblock__mod">${signed(a.modifier)}</span>`}</li>`,
    )
    .join('');
  const features = sheet.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join('');
  return (
    '<div class="kit-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body">' +
    `<h3 class="kit-statblock__name">${esc(name)}</h3><ul class="kit-statblock__vitals">${vitals}</ul>` +
    `<ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${features}</ul></div></div>`
  );
}

/** What the AI proposed: one description list, then the statblock of an NPC. */
function proposal(item: Item, p: Proposal): string {
  let list = `<dl class="mf-tocreate__proposal" id="to-create-${esc(item.id)}-proposal" data-reads="${GET_ENTITY}"><dt>Description</dt><dd>${esc(p.description)}</dd>`;
  if (p.aliases.length > 0) list += `<dt>Alias proposés</dt><dd>${esc(p.aliases.join(', '))}</dd>`;
  if (p.background !== '') list += `<dt>Background</dt><dd>${esc(p.background)}</dd>`;
  if (p.behavior !== '') list += `<dt>Comportement, pour l’interpréter</dt><dd>${esc(p.behavior)}</dd>`;
  list += '</dl>';
  return list + (p.sheet === null ? '' : statblock(p.name || item.name, p.sheet));
}

function micButton(item: Item, view: View): string {
  const id = `to-create-${esc(item.id)}-mic`;
  return (
    `<div class="kit-micbutton kit-micbutton--compact" id="${id}-box" data-state="${view.mic}">` +
    `<button type="button" class="kit-micbutton__btn" id="${id}" data-action="dictate" aria-pressed="${view.mic === 'recording'}" aria-label="Dicter la description — ${MIC_TEXTS[view.mic]}"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button>` +
    '<div class="kit-micbutton__text"><span class="kit-micbutton__label">Dicter la description</span>' +
    `<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state">${MIC_TEXTS[view.mic]}</span> <span class="kit-micbutton__timer">${clock(view.seconds)}</span></span></div></div>`
  );
}

function refineForm(item: Item, view: View, locked: boolean): string {
  const field = `to-create-${esc(item.id)}-text`;
  return (
    `<div class="mf-tocreate__refine" id="to-create-${esc(item.id)}-refine">` +
    micButton(item, view) +
    '<div class="kit-textarea is-wide">' +
    `<label class="kit-textarea__label" for="${field}">Description affinée</label>` +
    `<textarea class="kit-textarea__field" id="${field}" rows="3" placeholder="Ajoutez ce que le brief ne disait pas…"${locked ? ' disabled' : ''}>${esc(view.typed)}</textarea>` +
    '<div class="kit-textarea__footer"><p class="kit-textarea__hint"></p></div></div>' +
    buttons(
      [
        { action: 'propose-refined', label: 'Proposer', variant: 'primary', writes: CREATE_FROM_BRIEF },
        { action: 'cancel', label: 'Annuler', variant: 'ghost' },
      ],
      locked,
    ) +
    '</div>'
  );
}

const statusLine = (outcome: Outcome | null): string =>
  outcome === null ? '' : `<p class="item ${outcome.kind === 'error' ? 'is-error' : 'muted'}" aria-live="polite">${esc(outcome.text)}</p>`;

/** The fiche of a created entry: its own page when it has an element, a new sheet to fill otherwise. */
const sheetHref = (item: Item): string => (item.entity === null ? '' : `fiche-entite.html?id=${encodeURIComponent(item.entity)}`);

/** One entry: its name, its type, its session and its excerpt, then what the DM can do with it. */
export function itemHTML(item: Item, view: View, locked: boolean): string {
  const done = item.status === 'created';
  let body = `<p class="quote">${esc(item.excerpt)}</p>`;
  if (done) {
    body += view.proposal === null ? '' : proposal(item, view.proposal);
    if (item.entity !== null) body += `<div class="kit-button kit-button--bare kit-button--small"><a class="secondary" href="${sheetHref(item)}">Ouvrir la fiche</a></div>`;
  } else if (view.refining) {
    body += refineForm(item, view, locked);
  } else {
    body += buttons(
      [
        { action: 'from-brief', label: 'Créer depuis le brief', variant: 'primary', writes: CREATE_FROM_BRIEF },
        { action: 'refine', label: 'Affiner', variant: 'secondary' },
      ],
      locked,
    );
  }
  const badges = `<span class="accent">${esc(TYPES[item.type] ?? item.type)}</span>${done ? '<span class="success">Créé</span>' : '<span>À créer</span>'}`;
  return (
    `<li class="mf-tocreate__item${done ? ' is-created' : ''}" id="to-create-${esc(item.id)}" data-id="${esc(item.id)}">` +
    `<div class="panel__head"><h3>${esc(item.name)}</h3><div class="kit-badge">${badges}</div></div>` +
    `<p class="small muted">${esc(item.sessionTitle)}</p>${body}${statusLine(view.outcome)}</li>`
  );
}

/** The screen, its list empty: the title and the counter are there from the first paint. */
export function skeleton(): string {
  return (
    '<section class="panel mf-tocreate" id="to-create" aria-labelledby="to-create-title">' +
    '<div class="panel__head"><h2 id="to-create-title">Entités à créer</h2><span class="small muted" id="to-create-count"></span></div>' +
    '<p class="small muted">Issues des briefs de séance. Créez-les depuis le brief, ou affinez d’abord la description à l’oral ou à l’écrit.</p>' +
    `<p class="item is-error" id="to-create-error" hidden></p><ul class="mf-tocreate__list" id="to-create-items" data-reads="${LIST_ENTITIES_TO_CREATE}"></ul>` +
    '</section>'
  );
}
