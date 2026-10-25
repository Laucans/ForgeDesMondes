// The helpers of mock/src/commands.js, split by scripts/split-handlers.mjs: shared by the handler files that import them.
// What the mock server does with a DataCapability's command: one function
// per identifier, `(server, { payload, targetId, version, basedOn }) =>
// violations | undefined`. It judges the payload as the DataGuard would
// beyond the aggregate's own invariants (the server checks those from
// `fixtures/aggregates/<Aggregate>.json`, answer the extra violation ids
// here), changes the fixtures it loaded (`server.load(name)`), and answers
// nothing when applied; the server bumps `dataVersion` and every
// micro-frontend refetches. A DataCapability absent here is **rejected**
// with `unhandled-data-capability`, as the real server rejects what no
// manifest registers: list every DataCapability a page triggers.
//
//   'campagne.creerCampagne': async (server, { payload }) => {
//     if (!String(payload.name || '').trim()) return ['campaign-name-required'];
//     (await server.load('campagnes')).push({ id: server.uuid(), name: payload.name });
//   },

import { diffusionRow, instanceOf, ratioOfIllustration, shareInstance } from './reads.js';

// ProviderApiType@1 as the fixtures know it: the cloud host by itself, the
// home server at the URL whose model list the fixtures know; null otherwise.
export function apiType(settings, url) {
  let host = '';
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') host = parsed.hostname.toLowerCase();
  } catch { /* not a URL: not recognised */ }
  const home = settings.providers.opencode.url;
  return host === 'api.anthropic.com' ? 'cloud' : url !== '' && url === home ? 'opencode' : null;
}


// AliasMatch@1: the term without white space around it, in lowercase, with
// no accent (a combining mark U+0300 to U+036F of the decomposed letter).
export const normalized = (term) => String(term ?? '').trim().toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '').normalize('NFC');

// The fixture each kind of element is listed in.
export const LISTS = { pj: 'campaign.listCharacters', pnj: 'campaign.listCharacters', lieu: 'campaign.listPlaces', evenement: 'campaign.listEvents' };

// What a save writes of the editor's fields: these, and nothing else — a DEX
// modifier least of all, computed from the score (AbilityModifier@1).
export const SAVED = ['kind', 'name', 'aliases', 'description', 'notes', 'player', 'prompt', 'background', 'behavior', 'stats', 'ca', 'pvMax', 'spells', 'features', 'actions', 'portrait', 'token', 'crop', 'color'];

// The sources a save may name (EntityVersion@1), and the origin
// campaign.listEntityVersions words each one as: the DM's hand, or the AI's
// (which also reads the DM's brief).
export const SAVED_BY = { dm: 'manuelle', brief: 'ia', ia: 'ia' };

// The element `id` among the lists a save writes.
export const elementOf = async (server, id) => {
  for (const name of new Set(Object.values(LISTS))) {
    const element = ((await server.load(name)) || []).find((e) => e.id === id);
    if (element) return { element };
  }
  return {};
};

// A change by the AI or D&D Beyond: the fields set on the element, and a version of the history
// (EntityVersion@1) the DM has not seen.
export const changedBy = async (server, element, patch, origin, source, changes) => {
  Object.assign(element, patch);
  const history = (await server.load('campaign.listEntityVersions')) || {};
  const entry = history[element.id] || (history[element.id] = { name: element.name, versions: [] });
  const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
  entry.versions.unshift({ v, date: new Date().toISOString().slice(0, 19), origin, source, restoredFrom: null, seen: false, changes });
};

// A position on the screen, in fractions of it: from 0 to 1 — `kept` when
// what is asked is no number, which the server refuses.
export const onScreen = (fraction, kept) => (typeof fraction === 'number' && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept);

// The floating window of the diffusion, changed by `change`; the other tabs are told.
export async function onWindow(server, change) {
  const row = await diffusionRow(server);
  // window-fields-together: x, y and the width are all set while the window is open, none otherwise.
  if (!row.window) return ['window-fields-together'];
  const violations = await change(row.window);
  if (violations) return violations;
  if (window.Mock) window.Mock.store.set('diffusion', row);
  return undefined;
}

export const AROUND = [[1, 0], [1, -1], [0, -1], [-1, 0], [-1, 1], [0, 1]];

// The hexes a drop of `count` tokens fills: the dropped hex, then the free ones around it.
export function freeHexes(start, taken, count) {
  const busy = new Set(taken.map((t) => `${t.q},${t.r}`));
  const seen = new Set([`${start.q},${start.r}`]);
  const queue = [start];
  const out = [];
  for (let i = 0; i < queue.length && out.length < count; i += 1) {
    const hex = queue[i];
    if (!busy.has(`${hex.q},${hex.r}`)) out.push(hex);
    for (const [dq, dr] of AROUND) {
      const next = { q: hex.q + dq, r: hex.r + dr };
      if (!seen.has(`${next.q},${next.r}`)) { seen.add(`${next.q},${next.r}`); queue.push(next); }
    }
  }
  return out;
}

// One token of the instance a command targets, changed by `change`; the other tabs are told.
export async function onToken(server, target, token, change) {
  const data = await instanceOf(server, target && target.id);
  const found = data && data.state.tokens.find((t) => t.id === token);
  // instance-isolated: a token of another instance is refused.
  if (!found) return ['instance-isolated'];
  const violations = change(found, data.state);
  if (violations) return violations;
  shareInstance(data);
  return undefined;
}

// The DEX modifier of a character's sheet, floor((score - 10) / 2).
export function dexModifier(characters, id) {
  const sheet = characters.find((c) => c.id === id);
  return Math.floor(((sheet && sheet.stats ? sheet.stats.DEX : 10) - 10) / 2);
}

// next-turn: the turn passes to the next living participant by rank; the round counts up when the order comes back to its first.
export function advance(combat, state) {
  const { order } = combat;
  const from = Math.max(0, order.findIndex((o) => o.id === combat.current));
  for (let step = 1; step <= order.length; step += 1) {
    const at = (from + step) % order.length;
    const token = state.tokens.find((t) => t.id === order[at].id);
    if (token && !token.dead) {
      if (at <= from) combat.round += 1;
      combat.current = order[at].id;
      return;
    }
  }
}

// The combat of the instance a command targets, changed by `change`; refused when none is under way.
export async function onCombat(server, id, change) {
  const data = await instanceOf(server, id);
  if (!data) return ['instance-isolated'];
  if (!data.state.combat) return ['one-active-combat'];
  const violations = await change(data.state.combat, data.state);
  if (violations) return violations;
  shareInstance(data);
  return undefined;
}

// SoundTag@1: each tag trimmed of White_Space and lowercased character by
// character, an empty one dropped, then a tag seen earlier dropped — the
// first occurrence stays, the order is kept.
export const soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '')].map((c) => c.toLowerCase()).join('')).filter(Boolean))];
