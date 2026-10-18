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

import { diffusionRow, instanceOf, pristineState, ratioOfIllustration, shareInstance } from './reads.js';

// ProviderApiType@1 as the fixtures know it: the cloud host by itself, the
// home server at the URL whose model list the fixtures know; null otherwise.
function apiType(settings, url) {
  let host = '';
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') host = parsed.hostname.toLowerCase();
  } catch { /* not a URL: not recognised */ }
  const home = settings.providers.opencode.url;
  return host === 'api.anthropic.com' ? 'cloud' : url !== '' && url === home ? 'opencode' : null;
}

const AROUND = [[1, 0], [1, -1], [0, -1], [-1, 0], [-1, 1], [0, 1]];

// The hexes a drop of `count` tokens fills: the dropped hex, then the free ones around it.
function freeHexes(start, taken, count) {
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
async function onToken(server, target, token, change) {
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
function dexModifier(characters, id) {
  const sheet = characters.find((c) => c.id === id);
  return Math.floor(((sheet && sheet.stats ? sheet.stats.DEX : 10) - 10) / 2);
}

// next-turn: the turn passes to the next living participant by rank; the round counts up when the order comes back to its first.
function advance(combat, state) {
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
async function onCombat(server, id, change) {
  const data = await instanceOf(server, id);
  if (!data) return ['instance-isolated'];
  if (!data.state.combat) return ['one-active-combat'];
  const violations = await change(data.state.combat, data.state);
  if (violations) return violations;
  shareInstance(data);
  return undefined;
}

// The floating window of the diffusion, changed by `change`; the other tabs are told.
async function onWindow(server, change) {
  const row = await diffusionRow(server);
  // window-fields-together: x, y and the width are all set while the window is open, none otherwise.
  if (!row.window) return ['window-fields-together'];
  const violations = await change(row.window);
  if (violations) return violations;
  if (window.Mock) window.Mock.store.set('diffusion', row);
  return undefined;
}

export const COMMANDS = {
  // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone;
  // the size and the content stay.
  'live.moveWindow': (server, { payload }) => onWindow(server, (w) => { w.x = payload.x; w.y = payload.y; }),
  // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
  // is stored, the largest that fits the box asked for at the image's ratio
  // (window-proportional); the position stays.
  'live.resizeWindow': (server, { payload }) => onWindow(server, async (w) => {
    const ratio = await ratioOfIllustration(server, w.id || w.media);
    w.w = Math.max(0.1, Math.min(payload.w, payload.h * ratio, 1 - w.x));
  }),
  // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
  // one position both views read.
  'table.moveToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.q = payload.q; t.r = payload.r; }),
  // table.renameToken: only that token's name; an empty name is refused.
  'table.renameToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
    if (!String(payload.name || '').trim()) return ['name-required'];
    t.name = String(payload.name).trim();
    return undefined;
  }),
  // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
  'table.setTokenVisibility': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.visible = payload.visible === true; }),
  // table.setTokenCondition: the badges of that token alone.
  'table.setTokenCondition': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.states = [...(payload.conditions || [])]; }),
  // table.removeToken: the token leaves the map.
  'table.removeToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t, state) => { state.tokens = state.tokens.filter((x) => x !== t); }),
  // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
  // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
  // no hit points; the current ones stay within 0 and the maximum, and an NPC
  // is dead iff they are 0 (dead-derived).
  'table.changeHitPoints': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
    if (t.kind !== 'pnj' || typeof t.hp !== 'number') return ['pj-hp-not-tracked'];
    t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload.delta) || 0)));
    t.dead = t.hp === 0;
    return undefined;
  }),
  // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
  // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
  // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
  'table.startCombat': async (server, { payload, targetId }) => {
    const data = await instanceOf(server, targetId);
    if (!data) return ['instance-isolated'];
    if (data.state.combat) return ['one-active-combat'];
    const characters = (await server.load('campaign.listCharacters')) || [];
    const rolls = payload.rolls || {};
    const entered = payload.initiative || {};
    const order = [];
    for (const t of data.state.tokens.filter((x) => !x.dead)) {
      const dex = dexModifier(characters, t.char);
      if (t.kind === 'pnj') {
        const roll = Number(rolls[t.id]);
        // npc-initiative-bounds: a d20 between 1 and 20.
        if (!(roll >= 1 && roll <= 20)) return ['npc-initiative-bounds'];
        order.push({ id: t.id, init: roll + dex, dex, tie: Math.random() });
      } else {
        const init = Number(entered[t.char]);
        if (!(init >= 1)) return ['npc-initiative-bounds'];
        order.push({ id: t.id, init, dex, tie: Math.random() });
      }
    }
    if (!order.length) return ['one-active-combat'];
    order.sort((a, b) => b.init - a.init || b.dex - a.dex || b.tie - a.tie);
    data.pjInitiative = { ...data.pjInitiative, ...entered };
    data.state.combat = { round: 1, current: order[0].id, order: order.map(({ id, init, dex }) => ({ id, init, dex })) };
    shareInstance(data);
    return undefined;
  },
  // table.nextTurn: next-turn — the next living participant by rank, the round counting up when it comes back to the first.
  'table.nextTurn': (server, { targetId }) => onCombat(server, targetId, (combat, state) => { advance(combat, state); return undefined; }),
  // table.reorderInitiative: the participant takes the rank `position`; nobody else changes relative rank.
  'table.reorderInitiative': (server, { payload, targetId }) => onCombat(server, targetId, (combat) => {
    const from = combat.order.findIndex((o) => o.id === payload.token);
    if (from < 0) return ['instance-isolated'];
    const [moved] = combat.order.splice(from, 1);
    combat.order.splice(Math.max(0, Math.min(combat.order.length, Number(payload.position) || 0)), 0, moved);
    return undefined;
  }),
  // table.addParticipant: a token joins the running order at the place its initiative earns.
  'table.addParticipant': (server, { payload, targetId }) => onCombat(server, targetId, async (combat, state) => {
    const token = state.tokens.find((t) => t.id === payload.token);
    if (!token || combat.order.some((o) => o.id === token.id)) return ['instance-isolated'];
    const dex = dexModifier((await server.load('campaign.listCharacters')) || [], token.char);
    const roll = Number(payload.roll);
    const init = token.kind === 'pnj' ? roll + dex : Number(payload.initiative);
    if (!(init >= 1) || (token.kind === 'pnj' && !(roll >= 1 && roll <= 20))) return ['npc-initiative-bounds'];
    const at = combat.order.findIndex((o) => o.init < init || (o.init === init && o.dex < dex));
    combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
    return undefined;
  }),
  // table.removeParticipant: the token leaves the order, not the map; the turn passes on if it was its turn.
  'table.removeParticipant': (server, { payload, targetId }) => onCombat(server, targetId, (combat, state) => {
    if (!combat.order.some((o) => o.id === payload.token)) return ['instance-isolated'];
    const rest = { ...combat, order: combat.order.filter((o) => o.id !== payload.token) };
    if (combat.current === payload.token) {
      // The leaver counts as gone: the turn goes to the next living participant of those left.
      const gone = { ...state, tokens: state.tokens.map((t) => (t.id === payload.token ? { ...t, dead: true } : t)) };
      advance(combat, gone);
      rest.round = combat.round;
      rest.current = combat.current === payload.token ? (rest.order[0] && rest.order[0].id) || '' : combat.current;
    }
    if (rest.order.length) Object.assign(combat, rest); else state.combat = null;
    return undefined;
  }),
  // table.endCombat: the combat ends; the tokens stay as they are.
  'table.endCombat': (server, { targetId }) => onCombat(server, targetId, (combat, state) => { state.combat = null; return undefined; }),
  // table.resetBattlemap: state-persisted-reset — the instance goes back to the state it was opened with: no combat, no initiative.
  'table.resetBattlemap': async (server, { targetId }) => {
    const data = await instanceOf(server, targetId);
    if (!data) return ['instance-isolated'];
    data.state = pristineState(data.id);
    data.pjInitiative = {};
    shareInstance(data);
    return undefined;
  },
  // table.placeTokens: one character dropped, once or « × n »; the new tokens
  // are numbered after those already taken for it (multi-drop-numbering), a
  // monster takes its hit points from its sheet, a PC has one token.
  'table.placeTokens': async (server, { payload, targetId }) => {
    const data = await instanceOf(server, targetId);
    const character = ((await server.load('campaign.listCharacters')) || []).find((c) => c.id === payload.character);
    if (!data || !character) return ['instance-isolated'];
    const { tokens } = data.state;
    const mine = tokens.filter((t) => t.char === character.id);
    const pnj = character.kind === 'pnj';
    if (!pnj && mine.length) return ['token-roster'];
    const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
    const base = character.tokenName || String(character.name).split(' ')[0];
    let num = Math.max(0, ...mine.map((t) => t.num || 0));
    const numbered = pnj && (count > 1 || mine.length > 0);
    for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
      num += 1;
      tokens.push({ id: `t-${character.id}${pnj ? `-${num}` : ''}`, char: character.id, kind: character.kind, name: numbered ? `${base} ${num}` : base, num: pnj ? num : null,
        q: hex.q, r: hex.r, visible: true, hp: pnj ? character.pvMax : null, hpMax: pnj ? character.pvMax : null, states: [], dead: false });
    }
    shareInstance(data);
    return undefined;
  },
  // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
  // the battlemap becomes the player view's base, framed as the DM framed it.
  'live.showOnPlayerView': async (server, { payload }) => {
    const data = await instanceOf(server, payload.instance);
    // base-coherent: a battlemap base is an instance of the session.
    if (payload.kind !== 'battlemap' || !data) return ['base-coherent'];
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.base = { kind: 'battlemap', id: data.id };
      diffusion.view = { z: payload.zoom, px: payload.panX, py: payload.panY };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
  // ai.saveSettings (crates/ai/data-capabilities/save-settings): the DM's
  // provider saved and made active. The API type is derived from the URL,
  // never taken from the payload: the cloud host by itself, the home server
  // at the URL whose model list the fixtures know; any other URL is refused
  // by `api-type-recognized` (« type d'API non reconnu »). `binary: claude`
  // is stored « claude-code ». The key is kept masked, never echoed.
  'ai.saveSettings': async (server, { payload }) => {
    const settings = await server.load('ai.getSettings');
    const models = { generationModel: payload.generationModel, summaryModel: payload.summaryModel };
    // Both models are required: `single-settings-row` refuses a provider without them.
    if (!String(models.generationModel || '') || !String(models.summaryModel || '')) return ['single-settings-row'];
    if (payload.mode === 'api') {
      const url = String(payload.url || '');
      const type = apiType(settings, url);
      if (!type) return ['api-type-recognized'];
      const key = String(payload.key || '');
      settings.providers[type] = { ...settings.providers[type], url, ...models, key: key ? `••••${key.slice(-4)}` : '' };
      Object.assign(settings, { mode: 'api', provider: type, binary: null });
      return undefined;
    }
    if (payload.mode === 'binary') {
      const binary = { claude: 'claude-code', 'claude-code': 'claude-code', opencode: 'opencode' }[payload.binary];
      if (!binary) return ['mode-fields'];
      Object.assign(settings, { mode: 'binary', provider: binary, binary: { binary, ...models } });
      return undefined;
    }
    return ['mode-fields'];
  },
  // ai.testConnection (crates/ai/data-capabilities/test-connection): the
  // configuration on screen tested and nothing of it saved — only `lastTest`
  // changes (`test-does-not-save`). Its type is detected as a save's; a URL
  // of no known type is the active provider's failed test, « type d'API non
  // reconnu ». The key is never kept.
  'ai.testConnection': async (server, { payload }) => {
    const settings = await server.load('ai.getSettings');
    const at = new Date().toISOString();
    const provider = payload.mode === 'api'
      ? apiType(settings, String(payload.url || ''))
      : payload.mode === 'binary'
        ? { claude: 'claude-code', 'claude-code': 'claude-code', opencode: 'opencode' }[payload.binary] || null
        : null;
    if (!provider) {
      settings.lastTest = { ok: false, at, provider: settings.provider, latency: null, message: "type d'API non reconnu" };
      return undefined;
    }
    // An API provider answers as the fixtures say, its latency the one its
    // message gives; a binary, the provider of its own name, answers at once.
    const message = (payload.mode === 'api' && settings.testResults?.[provider]?.message)
      || `Connexion réussie (binaire ${provider})`;
    const latency = message.match(/répond en ([\d,]+ s)/)?.[1] || '0,01 s';
    settings.lastTest = { ok: true, at, provider, latency, message };
    return undefined;
  },
};
