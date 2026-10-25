// The pill's markup, from mock/components/SuggestionPill.html and the kit's
// MicButton (mock/kit/MicButton.html): the same ids, the same classes. Every
// part names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { LIST_SUGGESTIONS, PLAY_SOUND, SET_MICROPHONE } from './ids.ts';
import { timeOf } from './model.ts';
import type { MicState, Suggestion } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const MIC =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
const PAUSE =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';

/** The mic's states, in words. */
export const MIC_TEXTS: Record<MicState, string> = { idle: 'Éteint · transcription locale', recording: 'Écoute en local', paused: 'En pause' };
export const MIC_LABEL = 'Micro de séance';

/** The letter an element's type is drawn with. */
const ICON: Record<string, string> = { lieu: 'L', place: 'L', pnj: 'P', npc: 'P', pj: 'J', 'événement': 'É', evenement: 'É', event: 'É' };

/** The pill, its lists empty: what never depends on the suggestions — the title, the two toggles — is there from the first paint. */
export function skeleton(): string {
  return (
    '<aside class="mf-pill" id="suggestions" aria-labelledby="suggestions-title">' +
    '<div class="mf-pill__card" id="suggestions-card">' +
    '<div class="mf-pill__head"><h2 id="suggestions-title">Suggestions</h2>' +
    '<button type="button" class="icon-btn" id="suggestions-history-toggle" aria-expanded="false" aria-controls="suggestions-history" title="Historique des suggestions">⟲</button></div>' +
    '<p class="mf-pill__heard small" id="suggestions-heard" aria-live="polite"></p>' +
    '<p class="mf-pill__status small" id="suggestions-status" role="status" hidden></p>' +
    `<ol class="mf-pill__stack" id="suggestions-stack" data-reads="${LIST_SUGGESTIONS}"></ol>` +
    '<section class="mf-pill__history" id="suggestions-history" aria-labelledby="suggestions-history-title" hidden>' +
    '<h3 id="suggestions-history-title">Historique</h3>' +
    `<ol class="mf-pill__stack" id="suggestions-history-list" data-reads="${LIST_SUGGESTIONS}"></ol></section>` +
    '</div>' +
    `<div class="mf-pill__mic" id="suggestions-mic" data-writes="${SET_MICROPHONE}">` +
    '<div class="kit-micbutton kit-micbutton--compact" id="suggestions-mic-box" data-state="idle">' +
    `<button type="button" class="kit-micbutton__btn" id="suggestions-mic-button" aria-pressed="false" aria-label="${MIC_LABEL}"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button>` +
    `<div class="kit-micbutton__text"><span class="kit-micbutton__label">${MIC_LABEL}</span>` +
    '<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span></span></div></div>' +
    '</div></aside>'
  );
}

/** One suggestion: an element opens in the centre, a sound is armed by a first click and plays on the second. */
function row(s: Suggestion, index: number, prefix: string, armed: boolean): string {
  const sound = s.kind === 'sound';
  const at = timeOf(s.detectedAt);
  const icon = sound ? '♪' : (ICON[s.type.toLowerCase()] ?? '•');
  const hint = sound ? (armed ? 'Son · cliquer encore pour jouer' : 'Son · cliquer deux fois pour jouer') : esc(s.type);
  const attrs = sound ? ` data-writes="${PLAY_SOUND}" aria-pressed="${armed}"` : '';
  return (
    `<li><button type="button" id="${prefix}-${index}" class="${armed ? 'is-armed' : ''}" data-kind="${s.kind}"${attrs} data-type="${esc(s.type)}" data-id="${esc(s.id)}">` +
    `<span class="mf-pill__icon${sound ? ' is-sound' : ''}" aria-hidden="true">${icon}</span>` +
    `<span><b>${esc(s.name)}</b><small>${hint} · « ${esc(s.via)} »${at === '' ? '' : ` · ${at}`}</small></span></button></li>`
  );
}

/** A list of suggestions, or why it is empty. `armed` is the sound a first click is waiting on. */
export function rows(list: readonly Suggestion[], prefix: string, empty: string, armed: string | null): string {
  return list.length === 0 ? `<li class="mf-pill__empty">${esc(empty)}</li>` : list.map((s, i) => row(s, i, prefix, s.kind === 'sound' && s.id === armed)).join('');
}
