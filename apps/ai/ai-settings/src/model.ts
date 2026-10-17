// What the card knows, with no DOM: the settings as the Capability answers
// them, the form the DM edits, the commands that form becomes.

import { DATA_CAPABILITY_VERSION } from './ids.ts';
import type { ApiProvider, Binary, Form, LastTest, Models, Mode, Settings } from './types.ts';

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);

const models = (value: Record<string, unknown>): Models => ({ generationModel: text(value.generationModel), summaryModel: text(value.summaryModel) });

function entries<T>(value: unknown, read: (entry: Record<string, unknown>) => T): Record<string, T> {
  const out: Record<string, T> = {};
  if (isRecord(value)) for (const [name, entry] of Object.entries(value)) if (isRecord(entry)) out[name] = read(entry);
  return out;
}

function lastTest(value: unknown): LastTest | null {
  if (!isRecord(value) || typeof value.ok !== 'boolean') return null;
  return {
    ok: value.ok,
    at: text(value.at),
    provider: typeof value.provider === 'string' ? value.provider : null,
    latency: typeof value.latency === 'string' ? value.latency : null,
    message: text(value.message),
  };
}

/** The Capability's answer, however little of it there is: a store with no provider yet is an empty card, not an error. */
export function parseSettings(data: unknown): Settings {
  const answer = isRecord(data) ? data : {};
  const binary = isRecord(answer.binary) ? { binary: text(answer.binary.binary), ...models(answer.binary) } : null;
  return {
    mode: answer.mode === 'binary' ? 'binary' : 'api',
    provider: typeof answer.provider === 'string' ? answer.provider : null,
    binary,
    providers: entries(answer.providers, (p): ApiProvider => ({ label: text(p.label), url: text(p.url), key: text(p.key), ...models(p) })),
    binaries: entries(answer.binaries, (b) => ({ label: text(b.label), ...models(b) })),
    lastTest: lastTest(answer.lastTest),
    local: texts(answer.local),
    sent: texts(answer.sent),
  };
}

/** The name a binary is stored under: `claude` is Claude Code. */
const stored = (binary: Binary): string => (binary === 'claude' ? 'claude-code' : binary);

/** The models a binary starts with: its own row, else those of the API provider it speaks for. */
export function binaryModels(settings: Settings, binary: Binary): Models {
  const own = settings.binaries[stored(binary)] ?? (settings.binary?.binary === stored(binary) ? settings.binary : undefined);
  const api = settings.providers[binary === 'claude' ? 'cloud' : 'opencode'];
  const found = own ?? api;
  return { generationModel: found?.generationModel ?? '', summaryModel: found?.summaryModel ?? '' };
}

/** The form the card opens on: the active provider, as saved. */
export function initialForm(settings: Settings): Form {
  if (settings.mode === 'binary') {
    const binary: Binary = settings.provider === 'claude-code' ? 'claude' : 'opencode';
    return { mode: 'binary', binary, url: '', key: '', ...binaryModels(settings, binary) };
  }
  const active = settings.provider === null ? undefined : settings.providers[settings.provider];
  return { mode: 'api', binary: 'opencode', url: active?.url ?? '', key: '', generationModel: active?.generationModel ?? '', summaryModel: active?.summaryModel ?? '' };
}

/** The same form switched to `mode`: a binary brings its own models, API mode keeps what was typed. */
export function withMode(settings: Settings, form: Form, mode: Mode): Form {
  if (mode === form.mode) return form;
  return mode === 'binary' ? { ...form, mode, ...binaryModels(settings, form.binary) } : { ...form, mode };
}

export function withBinary(settings: Settings, form: Form, binary: Binary): Form {
  return { ...form, binary, ...binaryModels(settings, binary) };
}

/** The label of the API type the active provider has — « Type d’API détecté » — for the card as it was saved. */
export function savedTypeLabel(settings: Settings): string | null {
  if (settings.mode !== 'api' || settings.provider === null) return null;
  return settings.providers[settings.provider]?.label || null;
}

/** The label of the API type the last test reached, when it answered: only a recognised type answers. */
export function testedTypeLabel(settings: Settings): string | null {
  const test = settings.lastTest;
  return test?.ok && test.provider !== null ? settings.providers[test.provider]?.label || null : null;
}

const isHttp = (url: string): boolean => {
  try {
    return /^https?:$/.test(new URL(url).protocol);
  } catch {
    return false;
  }
};

/** Why the form cannot be sent, or nothing. The API type is the server's to detect: a URL is only checked for being one. */
export function problem(form: Form): string {
  if (form.mode === 'api' && !isHttp(form.url.trim())) return 'Renseignez une URL d’API valide (http ou https).';
  if (!form.generationModel.trim()) return 'Le modèle de génération est obligatoire.';
  if (!form.summaryModel.trim()) return 'Le modèle de résumé est obligatoire.';
  return '';
}

/** The payload of ai.saveSettings and ai.testConnection (data-capability.json): the API type is never given. */
export function payload(form: Form): Record<string, string> {
  const both = { generationModel: form.generationModel.trim(), summaryModel: form.summaryModel.trim() };
  return form.mode === 'api'
    ? { mode: 'api', url: form.url.trim(), key: form.key, ...both }
    : { mode: 'binary', binary: form.binary, ...both };
}

/** `POST /commands` for one DataCapability, by identifier. A test names no role; a save is the DM's own, to approve. */
export function commandBody(dataCapability: string, body: Record<string, string>, roles: string[]): unknown {
  return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, caller: { by: 'dm', from: 'AiSettings', roles } };
}

const VIOLATIONS: Record<string, string> = {
  'api-type-recognized': 'type d’API non reconnu',
  'mode-fields': 'les champs ne correspondent pas au mode choisi',
  'single-settings-row': 'les deux modèles sont obligatoires',
  'text-only-payload': 'seul du texte est accepté',
};

/** A rejected command's reasons, in words. */
export const reasons = (violations: readonly string[]): string => violations.map((v) => VIOLATIONS[v] ?? v).join(', ');

/** The statuses after which a command no longer moves. */
const FINAL = new Set(['applied', 'rejected', 'cancelled', 'expired']);

export interface Settled {
  status: string;
  violations: string[];
}

/** `{ result: { status, violations } }`, as the server answers a command and its lookup. */
export function settled(answer: unknown): Settled {
  const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
  return { status: text(result.status), violations: texts(result.violations) };
}

export const isFinal = (status: string): boolean => FINAL.has(status);

export function commandId(answer: unknown): string {
  return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : '') : '';
}
