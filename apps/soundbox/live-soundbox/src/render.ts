// The panel's markup, from mock/components/LiveSoundbox.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names the
// unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { LIST_SOUNDS, PLAY_SOUND, SET_MASTER_VOLUME, SET_SOUND_VOLUME, STOP_ALL } from './ids.ts';
import { seconds } from './model.ts';
import type { Sound } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** The kit's Slider, bare, for the master volume: the label, the value, the bounds. It waits, disabled, for the library to give the value. */
export function masterSlider(): string {
  return (
    '<div class="kit-slider kit-slider--bare"><div class="kit-slider__field" data-name="master"><div class="kit-slider__head">' +
    '<label class="kit-slider__label" for="soundbox-master-volume">Volume global</label>' +
    '<span class="kit-slider__value"><output class="kit-slider__out" for="soundbox-master-volume">0</output><span class="kit-slider__unit">%</span></span></div>' +
    '<input class="kit-slider__input" id="soundbox-master-volume" type="range" name="master" min="0" max="100" value="0" style="--fill: 0%" disabled>' +
    '<div class="kit-slider__bounds"><span>0 %</span><span>100 %</span></div></div></div>'
  );
}

/** The kit's Slider moved to `value`, in place: its thumb, its filled track and its readout. */
export function setSlider(slider: HTMLInputElement, value: number): void {
  slider.value = String(value);
  slider.style.setProperty('--fill', `${value}%`);
  const out = slider.closest('.kit-slider__field')?.querySelector('.kit-slider__out');
  if (out) out.textContent = String(value);
}

/** The panel, its regions empty: what never depends on the library — the title, the search, « Tout arrêter » — is there from the first paint. */
export function skeleton(): string {
  const reads = `data-reads="${LIST_SOUNDS}"`;
  return (
    '<section class="mf-live-sb" id="live-soundbox" aria-labelledby="soundbox-title">' +
    '<div class="panel__head"><h2 id="soundbox-title">Soundbox</h2>' +
    `<span class="small muted" id="soundbox-playing-count" ${reads} aria-live="polite"></span></div>` +
    '<div class="mf-live-sb__bar" id="soundbox-controls">' +
    '<div id="soundbox-search"><div class="kit-input"><label>Rechercher un son' +
    '<input type="search" id="soundbox-search-input" placeholder="porte, foule, corbeau…"></label></div></div>' +
    `<div id="soundbox-master" ${reads} data-writes="${SET_MASTER_VOLUME}">${masterSlider()}</div>` +
    '<div class="kit-button kit-button--bare">' +
    `<button type="button" class="danger" id="soundbox-stop-all" data-writes="${STOP_ALL}">Tout arrêter</button></div></div>` +
    `<div class="row" id="soundbox-tags" role="group" aria-label="Filtrer par tag" ${reads}></div>` +
    '<p class="small mf-live-sb__message" id="soundbox-message" role="status"></p>' +
    `<ul class="mf-live-sb__grid" id="soundbox-grid" ${reads}></ul>` +
    '</section>'
  );
}

/** « 2 sons en cours · le son sort uniquement de cette vue ». */
export function playingCount(count: number): string {
  const only = 'le son sort uniquement de cette vue';
  return count === 0 ? 'Le son sort uniquement de cette vue' : `${count} son${count > 1 ? 's' : ''} en cours · ${only}`;
}

/** An element id for a tag, one tag to one id: letters and digits as they are, every other character by its code. */
export const tagId = (tag: string): string => Array.from(tag).map((c) => (/[a-z0-9]/.test(c) ? c : `_${c.codePointAt(0)?.toString(16) ?? ''}_`)).join('');

/** The tag filter's chips; the ones in `chosen` are on. */
export function tagChips(tags: readonly string[], chosen: ReadonlySet<string>): string {
  return tags
    .map((tag) => `<button type="button" class="chip${chosen.has(tag) ? ' is-on' : ''}" aria-pressed="${chosen.has(tag)}" data-tag="${esc(tag)}" id="soundbox-tag-${tagId(tag)}">#${esc(tag)}</button>`)
    .join('');
}

/** One sound's box: the play button, its volume, the bar that fills while it plays. */
export function box(sound: Sound, playing: boolean): string {
  const id = esc(sound.id);
  return (
    `<li class="mf-live-sb__box${playing ? ' is-playing' : ''}" id="soundbox-${id}" data-id="${id}">` +
    `<button type="button" class="mf-live-sb__play" data-writes="${PLAY_SOUND}" data-play="${id}"><b>${esc(sound.name)}</b>` +
    `<small>${esc(sound.tags.join(' · '))}${seconds(sound.duration)}</small></button>` +
    `<label class="mf-live-sb__vol">Vol. <input type="range" min="0" max="100" value="${sound.volume}" data-writes="${SET_SOUND_VOLUME}" data-volume="${id}" aria-label="Volume de ${esc(sound.name)}"><span>${sound.volume}</span></label>` +
    '<span class="mf-live-sb__progress"></span></li>'
  );
}

/** The grid of the sounds that match. */
export function grid(sounds: readonly Sound[], playing: ReadonlySet<string>): string {
  return sounds.map((sound) => box(sound, playing.has(sound.id))).join('') || '<li class="muted">Aucun son ne correspond.</li>';
}
