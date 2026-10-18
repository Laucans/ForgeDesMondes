// The panel's markup, from mock/components/GridSettings.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { GET_BATTLEMAP, SAVE_GRID_SETTINGS } from './ids.ts';
import { COLORS } from './model.ts';
import type { Battlemap, Form, Outcome } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** The panel, its regions empty: what never depends on the battlemap — the title, the scale reminder, the buttons — is there from the first paint. */
export function skeleton(): string {
  const reads = `data-reads="${GET_BATTLEMAP}"`;
  return (
    '<section class="panel mf-grid" id="grid-settings" aria-labelledby="grid-title">' +
    '<div class="panel__head"><h2 id="grid-title">Réglage de la grille</h2>' +
    '<span class="small muted" id="grid-scale">1 hex = 5 pieds · cliquez deux hex de l’aperçu pour vérifier une distance</span></div>' +
    '<div class="mf-grid__layout">' +
    '<form class="stack" id="grid-form">' +
    `<div class="stack" id="grid-fields" ${reads}></div>` +
    '<div id="grid-actions"><div class="kit-button kit-button--bare">' +
    `<button type="button" class="primary" id="grid-save" data-writes="${SAVE_GRID_SETTINGS}" disabled>Enregistrer le réglage</button>` +
    '<button type="button" class="ghost" id="grid-reset" disabled>Annuler les changements</button>' +
    '</div></div>' +
    '<div class="mf-grid__result" id="grid-result" aria-live="polite"></div>' +
    '</form>' +
    `<div id="grid-preview" ${reads}></div>` +
    '</div></section>'
  );
}

/** What stands in the preview's place while there is no map to draw. */
export const notice = (words: string): string => `<p class="muted" id="grid-notice">${esc(words)}</p>`;

/** The bounds of a slider: the mock-up's, widened to hold the value the battlemap really has. */
const bounds = (value: number, min: number, max: number): { min: number; max: number } => ({ min: Math.min(min, Math.floor(value)), max: Math.max(max, Math.ceil(value)) });

interface Field {
  id: string;
  name: string;
  label: string;
  value: number;
  min: number;
  max: number;
  unit: string;
  step?: string;
}

/** The kit's Slider, one field: label, value, range, bounds. */
function slider(f: Field): string {
  const { min, max } = bounds(f.value, f.min, f.max);
  const fill = max === min ? 0 : ((f.value - min) / (max - min)) * 100;
  const step = f.step ? ` step="${f.step}"` : '';
  return (
    `<div class="kit-slider__field" data-name="${f.name}"><div class="kit-slider__head">` +
    `<label class="kit-slider__label" for="${f.id}">${esc(f.label)}</label>` +
    `<span class="kit-slider__value"><output class="kit-slider__out" for="${f.id}">${f.value}</output><span class="kit-slider__unit">${f.unit}</span></span></div>` +
    `<input class="kit-slider__input" id="${f.id}" type="range" name="${f.name}" min="${min}" max="${max}" value="${f.value}"${step} style="--fill: ${fill}%">` +
    `<div class="kit-slider__bounds"><span>${min} ${f.unit}</span><span>${max} ${f.unit}</span></div></div>`
  );
}

/** The fields of the form: four sliders, the orientation as the kit's Radio, the colour as the kit's Select. */
export function fields(form: Form): string {
  const radio = (value: string, label: string): string => `<label><input type="radio" name="grid-orient" value="${value}"${form.orient === value ? ' checked' : ''}><span>${label}</span></label>`;
  const known = COLORS.some((c) => c.value === form.color);
  const colors = known ? COLORS : [...COLORS, { value: form.color, label: form.color }];
  return (
    '<div class="kit-slider kit-slider--bare">' +
    slider({ id: 'grid-size', name: 'size', label: 'Taille d’un hex', value: form.size, min: 16, max: 60, unit: 'px', step: 'any' }) +
    slider({ id: 'grid-ox', name: 'ox', label: 'Décalage horizontal', value: form.ox, min: -30, max: 60, unit: 'px', step: 'any' }) +
    slider({ id: 'grid-oy', name: 'oy', label: 'Décalage vertical', value: form.oy, min: -30, max: 60, unit: 'px', step: 'any' }) +
    slider({ id: 'grid-opacity', name: 'opacity', label: 'Opacité', value: form.opacity, min: 5, max: 100, unit: '%', step: 'any' }) +
    '</div>' +
    `<div class="kit-radio"><fieldset id="grid-orient"><legend>Orientation</legend>${radio('pointy', 'Pointe en haut')}${radio('flat', 'Côté plat en haut')}</fieldset></div>` +
    '<div class="kit-select is-wide"><span id="grid-color-label">Couleur de la grille</span>' +
    `<select id="grid-color" name="color" aria-labelledby="grid-color-label">${colors.map((c) => `<option value="${esc(c.value)}"${c.value === form.color ? ' selected' : ''}>${esc(c.label)}</option>`).join('')}</select></div>`
  );
}

/** The title, with the battlemap's own. */
export const title = (map: Battlemap | null): string => (map === null ? 'Réglage de la grille' : `Réglage de la grille — ${map.title}`);

/** The result area: what the last save said. */
export function result(outcome: Outcome): string {
  switch (outcome.kind) {
    case 'none':
      return '';
    case 'busy':
      return `<p class="muted">${esc(outcome.text)}</p>`;
    case 'ok':
    case 'error':
      return `<p class="${outcome.kind === 'ok' ? 'is-ok' : 'is-error'}" id="grid-message">${esc(outcome.text)}</p>`;
  }
}
