// The Micro-UI's markup, from mock/components/OralBrief.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part
// names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { ANALYZE_BRIEF, GET_LAST_BRIEF } from './ids.ts';
import type { Brief, Missing, Outcome, Recognized, Scene } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const MIC =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
const PAUSE =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';

/** The mic's states, in words. */
export const MIC_TEXTS = { idle: 'Prêt à écouter', recording: 'J’écoute le brief', paused: 'En pause — reprenez ou analysez' } as const;
export type MicState = keyof typeof MIC_TEXTS;

/** The kit's Button: one actuator, naming the DataCapability it triggers when it triggers one. */
function button(p: { id: string; label: string; variant: string; writes?: string; disabled?: boolean; diff?: string }): string {
  const writes = p.writes === undefined ? '' : ` data-writes="${p.writes}"`;
  const diff = p.diff === undefined ? '' : ` data-diff="${esc(p.diff)}"`;
  return `<button type="button" class="${p.variant}" id="${p.id}"${writes}${diff}${p.disabled === true ? ' disabled' : ''}>${esc(p.label)}</button>`;
}

/** The kit's Badge, one label. */
const badge = (label: string, tone: string): string => `<span class="kit-badge"><span${tone === '' ? '' : ` class="${tone}"`}>${esc(label)}</span></span>`;

/** The screen, its regions empty: what never depends on the brief — the title, the text area, the buttons — is there from the first paint. */
export function skeleton(): string {
  return (
    '<section class="panel mf-brief" id="oral-brief" aria-labelledby="brief-title">' +
    `<div class="panel__head"><h2 id="brief-title">Brief oral</h2><span class="small muted" id="brief-last" data-reads="${GET_LAST_BRIEF}"></span></div>` +
    '<p class="small muted">Racontez la séance au micro, ou tapez-la. L’IA en tire les scènes, reconnaît les PNJ, lieux et événements existants et place les manquants dans la liste « à créer ».</p>' +
    '<div id="brief-input"><div class="kit-textarea is-wide">' +
    '<label class="kit-textarea__label" for="brief-text">Brief de la séance</label>' +
    '<textarea class="kit-textarea__field" id="brief-text" rows="5"></textarea>' +
    '<div class="kit-textarea__footer"><p class="kit-textarea__hint">Seul le texte part vers l’IA, jamais le son.</p></div></div></div>' +
    '<div class="row" id="brief-controls">' +
    '<div class="kit-micbutton" id="brief-mic-box" data-state="idle">' +
    `<button type="button" class="kit-micbutton__btn" id="brief-mic" aria-pressed="false" aria-label="Brief oral"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button>` +
    '<div class="kit-micbutton__text"><span class="kit-micbutton__label">Brief oral</span>' +
    '<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div>' +
    `<div class="kit-button kit-button--bare">${button({ id: 'brief-analyse', label: 'Terminer et analyser', variant: 'primary', writes: ANALYZE_BRIEF, disabled: true })}</div>` +
    '</div>' +
    '<div class="mf-brief__status" id="brief-status" aria-live="polite"></div>' +
    `<div class="mf-brief__result" id="brief-result" data-reads="${GET_LAST_BRIEF}"></div>` +
    '</section>'
  );
}

/** The last brief's transcript, one line per entry: text only, the kit's Card. */
const transcript = (lines: readonly string[], duration: string | null): string =>
  '<article class="kit-card" id="brief-transcript-card"><div class="body">' +
  `<h3>Transcription${duration === null ? '' : ` · ${esc(duration)}`}</h3>` +
  `<ol class="mf-brief__lines" id="brief-transcript">${lines.map((line) => `<li>${esc(line)}</li>`).join('')}</ol>` +
  '</div></article>';

const scenes = (list: readonly Scene[]): string =>
  `<section class="mf-brief__col" id="brief-scenes"><h3>${list.length} scène${list.length > 1 ? 's' : ''} créée${list.length > 1 ? 's' : ''}</h3><ol>` +
  list.map((s) => `<li><b>${esc(s.title)}</b><br><small>${esc(s.links.join(' · '))}</small></li>`).join('') +
  '</ol></section>';

const known = (x: Recognized): string =>
  `<li id="brief-known-${esc(x.id)}"><a href="fiche-entite.html?id=${esc(x.id)}">${esc(x.name)}</a> <small>${esc(x.type)}${x.heard === null ? '' : ` · entendu : « ${esc(x.heard)} »`}</small> ${badge('Existe', 'success')}` +
  (x.updated
    ? `<br><span class="mf-brief__upd">${badge('Mis à jour', 'accent')}<span class="kit-button kit-button--bare">${button({ id: `brief-diff-${x.id}`, label: 'Voir le diff', variant: 'ghost', diff: x.id })}</span></span>${x.change === '' ? '' : `<br><small>${esc(x.change)}</small>`}`
    : '') +
  '</li>';

const recognized = (list: readonly Recognized[]): string =>
  `<section class="mf-brief__col" id="brief-recognized"><h3>${list.length} entité${list.length > 1 ? 's' : ''} reconnue${list.length > 1 ? 's' : ''}</h3><ul>${list.map(known).join('')}</ul></section>`;

/** A missing element is only listed: it waits on the to-create list, nothing here creates it. */
const absent = (x: Missing): string => `<li id="brief-missing-${esc(x.id)}"><a href="#to-create-${esc(x.id)}">${esc(x.name)}</a> <small>${esc(x.type)}</small> ${badge('à créer', '')}</li>`;

const missing = (list: readonly Missing[]): string =>
  `<section class="mf-brief__col" id="brief-missing"><h3>${list.length} manquante${list.length > 1 ? 's' : ''} → « à créer »</h3><ul>${list.map(absent).join('')}</ul></section>`;

/** What the last brief left: its transcript, then the scenes, the recognised and the missing elements; a quiet line when there is none. */
export function result(brief: Brief): string {
  if (brief.lastRun === null) return '<p class="small muted" id="brief-none">Aucun brief analysé pour cette séance.</p>';
  return `${transcript(brief.transcript, brief.duration)}<div class="mf-brief__cols">${scenes(brief.scenes)}${recognized(brief.recognized)}${missing(brief.missing)}</div>`;
}

/** What the status line shows. */
export function status(outcome: Outcome): string {
  switch (outcome.kind) {
    case 'none':
      return '';
    case 'busy':
      return `<p class="item muted">${esc(outcome.text)}</p>`;
    case 'ok':
      return `<p class="item is-ok" id="brief-done">${esc(outcome.text)}</p>`;
    case 'error':
      return `<p class="item is-error" id="brief-error">${esc(outcome.text)}</p>`;
  }
}
