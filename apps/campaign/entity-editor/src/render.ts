// The sheet's markup, from mock/components/EntityEditor.html and the kit's
// own (mock/kit/<Name>.html): the same ids, the same classes. Every part
// names the unit it shows (`data-reads`) or triggers (`data-writes`) by
// identifier.

import { GET_ENTITY, IMPORT_CHARACTER, LIST_MEDIA, LIST_PLAYERS, PROPOSE_CHARACTER, RESYNC_CHARACTER, SAVE_ENTITY } from './ids.ts';
import { colorName, initialsOf, isCharacter, signed, tokenHasImage } from './model.ts';
import { ABILITIES, type Change, type Entity, type Media, type Player } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const MIC =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
const PAUSE =
  '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';

/** The mic's states, in words. */
export const MIC_TEXTS = { idle: 'Prêt à dicter', recording: 'Enregistrement', paused: 'En pause' } as const;
export type MicState = keyof typeof MIC_TEXTS;

export const KIND_LABELS = { pj: 'PJ', pnj: 'PNJ', lieu: 'Lieu', evenement: 'Événement' } as const;

/** The kit's Button: one actuator, naming the DataCapability it triggers when it triggers one. */
function button(p: { id: string; label: string; variant: string; writes?: string; disabled?: boolean }): string {
  const writes = p.writes === undefined ? '' : ` data-writes="${p.writes}"`;
  return `<button type="button" class="${p.variant}" id="${p.id}"${writes}${p.disabled === true ? ' disabled' : ''}>${esc(p.label)}</button>`;
}

const buttons = (small: boolean, ...items: string[]): string => `<div class="kit-button kit-button--bare${small ? ' kit-button--small' : ''}">${items.join('')}</div>`;

/** The kit's Input; `label` may end in the derived modifier, which the sheet repaints. */
function input(p: { id: string; label: string; value: string | number; type?: string; wide?: boolean; hint?: string; placeholder?: string; min?: number; max?: number }): string {
  const attrs = `${p.min === undefined ? '' : ` min="${p.min}"`}${p.max === undefined ? '' : ` max="${p.max}"`}${p.placeholder === undefined ? '' : ` placeholder="${esc(p.placeholder)}"`}`;
  return (
    `<div class="kit-input${p.wide === true ? ' is-wide' : ''}"><label>${esc(p.label)}` +
    `<input type="${p.type ?? 'text'}" id="${p.id}" value="${esc(String(p.value))}"${attrs}>` +
    `${p.hint === undefined ? '' : `<small>${esc(p.hint)}</small>`}</label></div>`
  );
}

/** The kit's TextArea. */
function textarea(p: { id: string; label: string; value: string; rows: number; max?: number; hint?: string; placeholder?: string }): string {
  return (
    '<div class="kit-textarea is-wide">' +
    `<label class="kit-textarea__label" for="${p.id}">${esc(p.label)}</label>` +
    `<textarea class="kit-textarea__field" id="${p.id}" rows="${p.rows}"${p.max === undefined ? '' : ` maxlength="${p.max}"`}${p.placeholder === undefined ? '' : ` placeholder="${esc(p.placeholder)}"`}>${esc(p.value)}</textarea>` +
    `<div class="kit-textarea__footer"><p class="kit-textarea__hint">${esc(p.hint ?? '')}</p>${p.max === undefined ? '' : `<span class="kit-textarea__counter" aria-live="polite">${p.value.length} / ${p.max}</span>`}</div></div>`
  );
}

/** The kit's Select, dressed on a native <select> (a field a browser and a screen reader know as a list of options). */
function select(p: { id: string; label: string; value: string; options: ReadonlyArray<{ value: string; label: string }> }): string {
  const options = p.options.map((o) => `<option value="${esc(o.value)}"${o.value === p.value ? ' selected' : ''}>${esc(o.label)}</option>`).join('');
  return `<div class="kit-select is-wide"><span id="${p.id}-label">${esc(p.label)}</span><select id="${p.id}" aria-labelledby="${p.id}-label">${options}</select></div>`;
}

/** The kit's Badge. */
const badges = (items: ReadonlyArray<{ label: string; tone: string }>): string => `<div class="kit-badge">${items.map((b) => `<span class="${b.tone}">${esc(b.label)}</span>`).join('')}</div>`;

/** The sheet, its regions empty: the bar, the notice and the save button are there from the first paint. */
export function skeleton(): string {
  return (
    `<div class="mf-entity" id="entity-editor" data-reads="${GET_ENTITY}">` +
    '<div class="mf-entity__unsaved" id="entity-unsaved" role="alert" hidden><p id="entity-unsaved-text"></p>' +
    buttons(
      true,
      button({ id: 'entity-unsaved-save', label: 'Enregistrer et continuer', variant: 'primary', writes: SAVE_ENTITY }),
      button({ id: 'entity-unsaved-discard', label: 'Abandonner', variant: 'secondary' }),
      button({ id: 'entity-unsaved-stay', label: 'Rester', variant: 'ghost' }),
    ) +
    '</div>' +
    `<div class="mf-entity__notice" id="entity-notice" role="status" data-reads="${GET_ENTITY}" hidden></div>` +
    '<header class="mf-entity__head" id="entity-head"></header>' +
    '<div class="layout-2"><div class="stack">' +
    '<form class="panel" id="entity-identity" aria-labelledby="entity-identity-title" onsubmit="return false"></form>' +
    '<section class="panel" id="entity-prompt" aria-labelledby="entity-prompt-title" hidden></section>' +
    '<section class="panel" id="entity-sheet" aria-labelledby="entity-sheet-title" hidden></section>' +
    '</div><div class="stack">' +
    '<section class="panel" id="entity-ddb" aria-labelledby="entity-ddb-title" hidden></section>' +
    '<section class="panel" id="entity-table" aria-labelledby="entity-table-title"></section>' +
    '</div></div>' +
    '<div class="mf-entity__status" id="entity-status" aria-live="polite"></div>' +
    `<div class="row" id="entity-footer">${buttons(false, button({ id: 'entity-save', label: 'Enregistrer la fiche', variant: 'primary', writes: SAVE_ENTITY }))}</div>` +
    '</div>'
  );
}

/** The portrait, the badges, the name. */
export function head(entity: Entity, player: string | null, isNew: boolean): string {
  const tones: Array<{ label: string; tone: string }> = [{ label: KIND_LABELS[entity.kind], tone: 'primary' }];
  if (player !== null) tones.push({ label: `Joué par ${player}`, tone: 'success' });
  if (isNew) tones.push({ label: 'Nouvelle fiche', tone: 'accent' });
  const initials = initialsOf(entity.name) || '?';
  return (
    `<div class="mf-entity__portrait" data-reads="${LIST_MEDIA}"><span class="mf-entity__glyph">${esc(initials)}</span></div>` +
    `<div class="mf-entity__title">${badges(tones)}<h1 id="entity-title">${esc(entity.name || 'Sans nom')}</h1>` +
    `<p class="muted" id="entity-subtitle">${esc(entity.aliases.join(', ') || 'Renseignez le nom, les alias et la description.')}</p></div>`
  );
}

/** The identity form: name, aliases, kind and player of a character, description. */
export function identity(entity: Entity, players: readonly Player[]): string {
  const character = isCharacter(entity.kind);
  let html =
    '<h2 id="entity-identity-title">Identité</h2><div class="mf-entity__grid">' +
    input({ id: 'entity-name', label: 'Nom', value: entity.name, wide: true }) +
    input({ id: 'entity-aliases', label: 'Alias', value: entity.aliases.join(', '), wide: true, hint: 'Séparés par des virgules — servent à la détection au micro' }) +
    '</div>';
  if (character) {
    const kind = (value: 'pnj' | 'pj', label: string, hint: string): string =>
      `<label><input type="radio" name="entity-kind" value="${value}"${entity.kind === value ? ' checked' : ''}><span>${label}</span><small>${hint}</small></label>`;
    html +=
      '<div class="mf-entity__grid"><div class="kit-radio"><fieldset id="entity-kind"><legend>Personnage</legend>' +
      kind('pnj', 'PNJ', 'joué par le MJ') +
      kind('pj', 'PJ', 'lié à un joueur') +
      '</fieldset></div>' +
      `<div id="entity-player-field" data-reads="${LIST_PLAYERS}"${entity.kind === 'pj' ? '' : ' hidden'}>` +
      select({ id: 'entity-player', label: 'Joueur', value: entity.player, options: [{ value: '', label: 'Choisir un joueur' }, ...players.map((p) => ({ value: p.id, label: p.name }))] }) +
      '</div></div>';
  }
  return html + textarea({ id: 'entity-description', label: 'Description', value: entity.description, rows: 3, max: 600 });
}

/** The initial prompt, dictated or typed, and what the AI proposes from it. A sheet not saved has no id to ask for. */
export function prompt(entity: Entity, canPropose: boolean): string {
  const pj = entity.kind === 'pj';
  return (
    '<div class="panel__head"><h2 id="entity-prompt-title">Aide à la création</h2><span class="small muted">Seul le texte transcrit part vers le fournisseur IA</span></div>' +
    `<p class="small muted">Racontez le personnage à voix haute : l’IA propose un background, un comportement pour l’interpréter${pj ? '' : ' et un statblock crédible'}. La fiche est modifiée et la version précédente reste dans l’historique.</p>` +
    '<div class="row"><div class="kit-micbutton kit-micbutton--compact" id="entity-prompt-mic-box" data-state="idle">' +
    `<button type="button" class="kit-micbutton__btn" id="entity-prompt-mic" aria-pressed="false" aria-label="Raconter le prompt initial"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button>` +
    '<div class="kit-micbutton__text"><span class="kit-micbutton__label">Raconter le prompt initial</span>' +
    '<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div></div>' +
    textarea({ id: 'entity-prompt-text', label: 'Prompt initial', value: entity.prompt, rows: 3, placeholder: 'Ex. : un gobelin marchand, nerveux, qui doit de l’argent à des gens dangereux…', hint: canPropose ? '' : 'Enregistrez la fiche pour demander une proposition à l’IA.' }) +
    buttons(false, button({ id: 'entity-propose', label: pj ? 'Proposer background et comportement' : 'Proposer background, comportement et statblock', variant: 'primary', writes: PROPOSE_CHARACTER, disabled: !canPropose })) +
    textarea({ id: 'entity-background', label: 'Background', value: entity.background, rows: 4 }) +
    textarea({ id: 'entity-behavior', label: 'Comportement, pour l’interpréter', value: entity.behavior, rows: 3 })
  );
}

/** The sorts and capacités, one per line, each removable. */
export const features = (entity: Entity): string =>
  entity.features
    .map((f, i) => `<li data-feature="${i}"><span><b>${esc(f.name)}.</b> ${esc(f.text)}</span><button type="button" class="icon-btn" data-remove-feature="${i}" aria-label="Retirer ${esc(f.name)}">×</button></li>`)
    .join('');

/** The kit's StatBlock of an NPC, from what the sheet holds now. The modifiers are computed here (dex-modifier-derived). */
export function statblock(entity: Entity): string {
  const abilities = ABILITIES.map(
    (k) => `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${k}</span><span class="kit-statblock__score">${entity.stats[k]}</span><span class="kit-statblock__mod">${signed(entity.stats[k])}</span></li>`,
  ).join('');
  const vitals = [
    { label: 'CA', value: entity.ca },
    { label: 'PV', value: entity.pvMax },
  ]
    .map((v) => `<div class="kit-statblock__vital"><dt>${v.label}</dt><dd>${v.value}</dd></div>`)
    .join('');
  const list = entity.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join('');
  return (
    '<div class="kit-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body">' +
    `<h3 class="kit-statblock__name">${esc(entity.name || 'Sans nom')}</h3><p class="kit-statblock__type">${KIND_LABELS[entity.kind]}</p>` +
    `<dl class="kit-statblock__vitals">${vitals}</dl><ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${list}</ul></div></div>`
  );
}

/** The stats of a PC or an NPC: the six scores with their modifier, armour class, hit points, the features. */
export function sheet(entity: Entity): string {
  const pj = entity.kind === 'pj';
  return (
    `<div class="panel__head"><h2 id="entity-sheet-title">Fiche</h2><span class="small muted">${pj ? 'Mise à jour par D&amp;D Beyond' : 'Sert à l’initiative et aux PV en combat'}</span></div>` +
    `<div class="mf-entity__abilities" id="entity-abilities">${ABILITIES.map((k) => input({ id: `entity-stat-${k}`, label: `${k} ${signed(entity.stats[k])}`, type: 'number', value: entity.stats[k], min: 1, max: 30 })).join('')}</div>` +
    `<div class="mf-entity__grid">${input({ id: 'entity-ca', label: 'CA', type: 'number', value: entity.ca })}${input({ id: 'entity-pv', label: 'PV max', type: 'number', value: entity.pvMax })}</div>` +
    `<h3 id="entity-features-title">Sorts et capacités</h3><ul class="mf-entity__features" id="entity-features">${features(entity)}</ul>` +
    `<div class="mf-entity__grid" id="entity-feature-add">${input({ id: 'entity-feature-name', label: 'Nom', value: '', placeholder: 'Boule de feu' })}${input({ id: 'entity-feature-text', label: 'Description', value: '', placeholder: 'Sphère de 6 m, 8d6 dégâts de feu…', wide: true })}</div>` +
    buttons(true, button({ id: 'entity-feature-submit', label: 'Ajouter le sort ou la capacité', variant: 'secondary' })) +
    (pj ? '' : `<div id="entity-statblock">${statblock(entity)}</div>`)
  );
}

/** The kit's DiffView of what the resync changed. */
export function diffView(title: string, changes: readonly Change[]): string {
  const rows = changes
    .map(
      (c, i) =>
        `<li class="kit-diffview__row" data-index="${i}"><div class="kit-diffview__field">${esc(c.field)}</div><div class="kit-diffview__values">` +
        `<del class="kit-diffview__old">${esc(c.old || '—')}</del><ins class="kit-diffview__new">${esc(c.new || '—')}</ins></div></li>`,
    )
    .join('');
  return `<div class="kit-diffview"><header class="kit-diffview__head"><h2 class="kit-diffview__title">${esc(title)}</h2><span class="kit-diffview__meta">${changes.length} champ(s) mis à jour</span></header><ul class="kit-diffview__list">${rows}</ul></div>`;
}

/** What a resync left alone: the aliases, the notes and the token the DM defined here. */
export const kept = (entity: Entity): string =>
  '<h3>Conservés</h3><ul class="mf-entity__kept" id="entity-ddb-kept">' +
  `<li>Alias : ${esc(entity.aliases.join(', '))}</li><li>Notes : ${esc(entity.notes.slice(0, 60))}${entity.notes.length > 60 ? '…' : ''}</li><li>Token : recadrage ${entity.crop.zoom} %</li></ul>`;

const when = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString('fr-FR', { dateStyle: 'long', timeStyle: 'short' });
};

/** The D&D Beyond panel of a PC: the link, import, resync, the last synchronisation. */
export function ddb(entity: Entity, url: string): string {
  const linked = entity.ddb !== null;
  const syncedAt = entity.ddb?.syncedAt ?? null;
  return (
    '<h2 id="entity-ddb-title">D&amp;D Beyond</h2>' +
    input({ id: 'entity-ddb-url', label: 'URL du personnage', value: url, placeholder: 'https://www.dndbeyond.com/characters/…', wide: true }) +
    buttons(
      true,
      button({ id: 'entity-ddb-import', label: 'Importer', variant: linked ? 'ghost' : 'primary', writes: IMPORT_CHARACTER }),
      button({ id: 'entity-ddb-resync', label: 'Resynchroniser', variant: 'primary', writes: RESYNC_CHARACTER, disabled: !linked }),
    ) +
    `<p class="small muted" id="entity-ddb-status">${syncedAt === null ? 'Jamais importé' : `Dernière synchronisation : ${esc(when(syncedAt))}`}</p>` +
    '<p class="small muted">La resynchronisation met à jour les stats, les PV et les sorts. Les alias, les notes et le token définis ici sont conservés.</p><div id="entity-ddb-diff"></div>'
  );
}

/** The kit's Token: the disc of the colour with the initials, or the portrait's disc (default-token, TokenAppearance@1). */
export function token(entity: Entity): string {
  const initials = esc(initialsOf(entity.name));
  const clip = tokenHasImage(entity)
    ? `<span class="kit-token__portrait"><span class="mf-entity__glyph">${initials}</span></span>`
    : `<span class="kit-token__initials">${initials}</span>`;
  const first = entity.name.trim().split(/\s+/)[0] ?? '';
  return (
    '<div class="kit-token"><div class="kit-token__item">' +
    `<div class="kit-token__disc" style="--kit-token-bg: var(--color-${colorName(entity.color)})"><div class="kit-token__clip">${clip}</div>` +
    `${entity.kind === 'pnj' ? '<span class="kit-token__num">1</span>' : ''}</div><div class="kit-token__name">${esc(first)}</div></div></div>`
  );
}

/** The kit's Slider, bare: the three fields of the token's crop. */
function sliders(entity: Entity): string {
  const field = (id: string, name: string, label: string, value: number, min: number, max: number): string =>
    `<div class="kit-slider__field" data-name="${name}"><div class="kit-slider__head"><label class="kit-slider__label" for="${id}">${label}</label>` +
    `<span class="kit-slider__value"><output class="kit-slider__out" for="${id}">${value}</output><span class="kit-slider__unit">%</span></span></div>` +
    `<input class="kit-slider__input" id="${id}" type="range" name="${name}" min="${min}" max="${max}" value="${value}">` +
    `<div class="kit-slider__bounds"><span>${min} %</span><span>${max} %</span></div></div>`;
  return (
    '<div class="kit-slider kit-slider--bare">' +
    field('entity-crop-zoom', 'zoom', 'Recadrage — zoom', entity.crop.zoom, 100, 220) +
    field('entity-crop-x', 'x', 'Centre horizontal', entity.crop.x, 0, 100) +
    field('entity-crop-y', 'y', 'Centre vertical', entity.crop.y, 0, 100) +
    '</div>'
  );
}

/** The select of an image of the library: unchanged by default, so a choice is always the DM's. */
function mediaSelect(id: string, label: string, has: boolean, chosen: string | null | undefined, medias: readonly Media[]): string {
  const value = chosen === undefined ? '' : (chosen ?? 'none');
  const options = [{ value: '', label: has ? 'Image actuelle (conservée)' : 'Aucune image' }, ...(has ? [{ value: 'none', label: 'Retirer l’image' }] : []), ...medias.map((m) => ({ value: m.id, label: m.title }))];
  return select({ id, label, value, options });
}

/** « À la table » of a character: the portrait and the token image from the library, the token, the notes. A place or an event: the library's count. */
export function table(entity: Entity, illustrations: readonly Media[]): string {
  if (!isCharacter(entity.kind)) {
    const heading = entity.kind === 'lieu' ? 'Illustration' : 'Liens';
    const body =
      entity.kind === 'lieu'
        ? `<p class="muted" data-reads="${LIST_MEDIA}" id="entity-illustrations">${illustrations.length} illustration${illustrations.length > 1 ? 's' : ''} dans la médiathèque de la campagne.</p>`
        : '<p class="muted">Un événement se lie aux scènes depuis la préparation de séance.</p>';
    return `<h2 id="entity-table-title">${heading}</h2>${body}` + textarea({ id: 'entity-notes', label: 'Notes du MJ', value: entity.notes, rows: 4, hint: 'Jamais visibles sur la player view' });
  }
  return (
    '<h2 id="entity-table-title">À la table</h2>' +
    `<div data-reads="${LIST_MEDIA}" id="entity-media">${mediaSelect('entity-portrait', 'Portrait', entity.portrait, entity.portraitMedia, illustrations)}</div>` +
    `<h3>Token</h3><div class="mf-entity__token" id="entity-token"><div id="entity-token-preview">${token(entity)}</div>${sliders(entity)}</div>` +
    mediaSelect('entity-token-image', 'Image de token dédiée', entity.tokenImage, entity.tokenMedia, illustrations) +
    textarea({ id: 'entity-notes', label: 'Notes du MJ', value: entity.notes, rows: 4, hint: 'Jamais visibles sur la player view' })
  );
}

/** The `Mis à jour` notice: where the change came from, what moved, and the way to its diff. */
export function notice(source: string, changes: readonly Change[]): string {
  const fields = changes.map((c) => c.field).join(', ');
  return (
    `<span><b>Mis à jour</b> — ${esc(source)}${fields === '' ? '' : ` : ${esc(fields)}`}.</span>` +
    buttons(true, button({ id: 'entity-notice-diff', label: 'Voir le diff', variant: 'ghost' }))
  );
}
