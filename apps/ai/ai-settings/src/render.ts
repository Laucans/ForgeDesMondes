// The card's markup, from mock/components/AiSettings.html and the kit's own
// (mock/kit/<Name>.html): the same ids, the same classes. Every part names
// the unit it shows (`data-reads`) or triggers (`data-writes`) by identifier.

import { GET_SETTINGS, SAVE_SETTINGS, TEST_CONNECTION } from './ids.ts';
import type { Form, LastTest, Outcome, Settings } from './types.ts';

export const esc = (value: string): string => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const attr = (name: string, value: string): string => (value === '' ? '' : ` ${name}="${esc(value)}"`);

function input(p: { id: string; label: string; value: string; hint: string; type?: string; placeholder?: string }): string {
  return (
    `<div class="kit-input is-wide"><label>${esc(p.label)}` +
    `<input type="${p.type ?? 'text'}" id="${p.id}"${attr('value', p.value)}${attr('placeholder', p.placeholder ?? '')}>` +
    `<small>${esc(p.hint)}</small></label></div>`
  );
}

/** The card, its regions empty: what never depends on the settings — the title, the buttons, the reminder — is there from the first paint. */
export function skeleton(): string {
  const reads = `data-reads="${GET_SETTINGS}"`;
  return (
    '<div class="mf-ai layout-2" id="ai-settings">' +
    '<form class="panel" id="ai-settings-form" aria-labelledby="ai-settings-title">' +
    '<h2 id="ai-settings-title">Fournisseur de génération</h2>' +
    `<div id="ai-provider" ${reads}></div>` +
    `<div class="mf-ai__fields" id="ai-fields" ${reads}></div>` +
    `<div class="mf-ai__detected" id="ai-detected" ${reads} aria-live="polite"></div>` +
    '<div class="row" id="ai-actions"><div class="kit-button kit-button--bare">' +
    `<button type="button" class="secondary" id="ai-test" data-writes="${TEST_CONNECTION}">Tester la connexion</button>` +
    `<button type="button" class="primary" id="ai-save" data-writes="${SAVE_SETTINGS}">Enregistrer</button>` +
    '</div></div>' +
    `<div class="mf-ai__result" id="ai-test-result" ${reads} aria-live="polite"></div>` +
    '</form>' +
    '<aside class="panel" id="ai-privacy" aria-labelledby="ai-privacy-title">' +
    '<h2 id="ai-privacy-title">Ce qui sort de la machine</h2>' +
    '<p class="muted">La transcription et la détection restent locales. Seul du texte part vers le fournisseur choisi.</p>' +
    '<section id="ai-privacy-local" class="stack"><h3>Reste sur cette machine</h3>' +
    `<ul class="list" id="ai-local-list" ${reads}></ul></section>` +
    '<section id="ai-privacy-sent" class="stack"><h3>Part vers le fournisseur, en texte</h3>' +
    `<ul class="list" id="ai-sent-list" ${reads}></ul></section>` +
    '</aside></div>'
  );
}

/** The kit's Radio: API or binary. */
export function modeChoice(form: Form): string {
  const option = (value: string, label: string, hint: string): string =>
    `<label><input type="radio" name="ai-mode" value="${value}"${form.mode === value ? ' checked' : ''}><span>${label}</span><small>${esc(hint)}</small></label>`;
  return (
    '<div class="kit-radio"><fieldset id="ai-mode-choice"><legend>Mode de connexion</legend>' +
    option('api', 'API', 'une URL, le type d’API est détecté') +
    option('binary', 'Binaire', 'un programme sur cette machine') +
    '</fieldset></div>'
  );
}

/** The saved key, masked, when the URL on screen is the saved provider's: the only key the screen may suggest. */
export function keyPlaceholder(form: Form, settings: Settings): string {
  const saved = settings.provider === null ? undefined : settings.providers[settings.provider];
  return (saved !== undefined && saved.url === form.url.trim() && saved.key) || 'Aucune clé';
}

/** The models, and what comes before them: the URL and the key in API mode, the binary in binary mode. */
export function fields(form: Form, settings: Settings): string {
  const models =
    input({ id: 'ai-generation-model', label: 'Modèle de génération', value: form.generationModel, hint: 'Statblocks, backgrounds, briefs et notes' }) +
    input({ id: 'ai-summary-model', label: 'Modèle de résumé', value: form.summaryModel, hint: 'Résumé du journal à la clôture de séance' });
  if (form.mode === 'api') {
    return (
      input({ id: 'ai-url', label: 'URL de l’API', value: form.url, hint: 'Le type d’API (serveur maison ou cloud) est détecté à partir de l’URL' }) +
      input({ id: 'ai-key', label: 'Clé d’API', value: form.key, type: 'password', placeholder: keyPlaceholder(form, settings), hint: 'Facultative selon l’API. Stockée sur cette machine uniquement, jamais renvoyée à l’écran : à saisir de nouveau pour tester ou enregistrer' }) +
      models
    );
  }
  const option = (value: string, label: string): string => `<option value="${value}"${form.binary === value ? ' selected' : ''}>${label}</option>`;
  return (
    '<div class="kit-select is-wide"><span id="ai-binary-label">Binaire</span>' +
    `<select id="ai-binary" aria-labelledby="ai-binary-label">${option('opencode', 'opencode')}${option('claude', 'Claude Code')}</select>` +
    '<small>Programme lancé sur cette machine</small></div>' +
    models
  );
}

/** « Type d’API détecté », as the kit's Badge. */
export function detected(label: string | null): string {
  return label === null ? '' : `<div class="kit-badge"><span class="success" id="ai-detected-badge">Type d’API détecté : ${esc(label)}</span></div>`;
}

const when = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString('fr-FR', { dateStyle: 'long', timeStyle: 'short' });
};

/** The last test, as it was recorded. */
export function lastTestText(test: LastTest): string {
  return test.ok
    ? `Dernier test réussi le ${when(test.at)}${test.latency === null ? '' : ` — réponse en ${test.latency}`}.`
    : `Dernier test échoué le ${when(test.at)} : ${test.message || 'connexion impossible'}`;
}

/** What the result area shows: the test just made, the last one on record, or nothing. */
export function result(outcome: Outcome): string {
  switch (outcome.kind) {
    case 'none':
      return '';
    case 'busy':
      return `<p class="muted">${esc(outcome.text)}</p>`;
    case 'last':
      return `<p class="${outcome.test.ok ? 'is-ok' : 'is-error'}" id="ai-test-message">${esc(lastTestText(outcome.test))}</p>`;
    case 'ok':
    case 'error':
      return `<p class="${outcome.kind === 'ok' ? 'is-ok' : 'is-error'}" id="ai-test-message">${esc(outcome.text)}</p>`;
  }
}

export const items = (list: readonly string[]): string => list.map((item) => `<li>${esc(item)}</li>`).join('');
