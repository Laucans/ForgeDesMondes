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
function apiType(settings, url) {
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
const normalized = (term) => String(term ?? '').trim().toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '').normalize('NFC');

// The fixture each kind of element is listed in.
const LISTS = { pj: 'campaign.listCharacters', pnj: 'campaign.listCharacters', lieu: 'campaign.listPlaces', evenement: 'campaign.listEvents' };

// What a save writes of the editor's fields: these, and nothing else — a DEX
// modifier least of all, computed from the score (AbilityModifier@1).
const SAVED = ['kind', 'name', 'aliases', 'description', 'notes', 'player', 'prompt', 'background', 'behavior', 'stats', 'ca', 'pvMax', 'spells', 'features', 'actions', 'portrait', 'token', 'crop', 'color'];

// A position on the screen, in fractions of it: from 0 to 1 — `kept` when
// what is asked is no number, which the server refuses.
const onScreen = (fraction, kept) => (typeof fraction === 'number' && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept);

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

// SoundTag@1: each tag trimmed of White_Space and lowercased character by
// character, an empty one dropped, then a tag seen earlier dropped — the
// first occurrence stays, the order is kept.
const soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '')].map((c) => c.toLowerCase()).join('')).filter(Boolean))];


export const COMMANDS = {
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
  // table.setTokenCondition (crates/table/data-capabilities/set-token-condition): the badge
  // `payload.state` put on that token when `payload.on`, after the others and once, or taken off,
  // the others keeping their order — its conditions alone. A state that is no name without the
  // blanks around it, or holds U+0000, or an `on` that is no boolean, is `state-persisted-reset`.
  'table.setTokenCondition': (server, { payload, targetId }) => onToken(server, { id: targetId || payload.instance }, payload.token, (t) => {
    const state = typeof payload.state === 'string' ? payload.state.trim() : '';
    if (!state || state.includes('\0') || typeof payload.on !== 'boolean') return ['state-persisted-reset'];
    const held = t.states || [];
    t.states = payload.on ? (held.includes(state) ? held : [...held, state]) : held.filter((s) => s !== state);
    return undefined;
  }),
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
  // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative): the open combat
  // reordered as `payload.order` gives it, every participant once, named by its token — the ranks
  // 1 to n in that order; the turn and the round stay. An order that is no list, omits a
  // participant, repeats one or names one not in the combat is `initiative-tiebreak`.
  'table.reorderInitiative': (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, (combat) => {
    const named = Array.isArray(payload.order) ? payload.order : [];
    if (named.length !== combat.order.length) return ['initiative-tiebreak'];
    const order = [];
    for (const id of named) {
      const found = combat.order.find((o) => o.id === id && !order.includes(o));
      if (!found) return ['initiative-tiebreak'];
      order.push(found);
    }
    combat.order = order;
    return undefined;
  }),
  // table.addParticipant (crates/table/data-capabilities/add-participant): a token joins the running
  // order at the place its initiative earns. An NPC rolls a d20 plus its DEX modifier and is given no
  // figure; a PC takes the `payload.initiative` the DM entered (`npc-initiative-bounds`). A token not
  // on the instance is `not-found`; one already in the order is `initiative-tiebreak`.
  'table.addParticipant': (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, async (combat, state) => {
    const token = state.tokens.find((t) => t.id === payload.token);
    if (!token) return ['not-found'];
    if (combat.order.some((o) => o.id === token.id)) return ['initiative-tiebreak'];
    const pc = token.kind === 'pj';
    if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ['npc-initiative-bounds'];
    const dex = dexModifier((await server.load('campaign.listCharacters')) || [], token.char);
    const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
    const at = combat.order.findIndex((o) => o.init < init || (o.init === init && o.dex < dex));
    combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
    return undefined;
  }),
  // table.removeParticipant (crates/table/data-capabilities/remove-participant): the participant
  // `payload.participant` — named by its token, as the carousel shows it — taken out of the open combat;
  // its token stays on the map and the order closes up. The participant that holds the turn is refused
  // by `next-turn`: the Battlemap sends table.nextTurn first. One not in the combat is `not-found`.
  'table.removeParticipant': (server, { payload, targetId }) => onCombat(server, targetId, (combat) => {
    if (!combat.order.some((o) => o.id === payload.participant)) return ['not-found'];
    if (combat.current === payload.participant) return ['next-turn'];
    combat.order = combat.order.filter((o) => o.id !== payload.participant);
    return undefined;
  }),
  // table.endCombat: the combat ends; the tokens stay as they are.
  'table.endCombat': (server, { targetId }) => onCombat(server, targetId, (combat, state) => { state.combat = null; return undefined; }),
  // table.resetBattlemap: state-persisted-reset — the instance's tokens, combat and initiative are removed: an empty map.
  'table.resetBattlemap': async (server, { targetId }) => {
    const data = await instanceOf(server, targetId);
    if (!data) return ['instance-isolated'];
    data.state = { tokens: [], combat: null };
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
    // token-roster: a PC is placed when the scene's roster has it, and once.
    if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ['token-roster'];
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
  // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone,
  // brought back onto the screen (0 to 1); the size and the content stay.
  'live.moveWindow': (server, { payload }) => onWindow(server, (w) => { w.x = onScreen(payload.x, w.x); w.y = onScreen(payload.y, w.y); }),
  // live.closeWindow (crates/live/data-capabilities/close-window): the window's
  // media, position and width are emptied together (window-fields-together);
  // the base and the framing stay. A window already closed is closed again.
  'live.closeWindow': async (server) => {
    const row = await diffusionRow(server);
    row.window = null;
    if (window.Mock) window.Mock.store.set('diffusion', row);
    return undefined;
  },
  // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
  // is stored, the largest that fits the box asked for at the image's ratio — w
  // alone with no h — from 0.1 to the screen's width, to 4 decimals
  // (window-proportional); the position stays. A w that is no number, which the
  // server refuses, keeps the width.
  'live.resizeWindow': (server, { payload }) => onWindow(server, async (w) => {
    if (typeof payload.w !== 'number' || !Number.isFinite(payload.w)) return undefined;
    const ratio = await ratioOfIllustration(server, w.id || w.media);
    const fit = typeof payload.h === 'number' && Number.isFinite(payload.h) ? Math.min(payload.w, payload.h * ratio) : payload.w;
    w.w = Math.round(Math.max(0.1, Math.min(1, fit)) * 10000) / 10000;
    return undefined;
  }),
  // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
  // a battlemap instance of the session, or an illustration of the campaign,
  // becomes the player view's base, framed as the DM framed it (1 and 0, 0
  // when no framing is sent); the floating window stays as it is. Any other
  // base would leave it incoherent (`base-coherent`).
  'live.showOnPlayerView': async (server, { payload }) => {
    let base;
    if (payload.kind === 'battlemap') {
      const data = await instanceOf(server, payload.instance);
      if (!data) return ['base-coherent'];
      base = { kind: 'battlemap', id: data.id };
    } else if (payload.kind === 'illustration') {
      // An illustration listed, or one uploaded since (`media.uploadMedia`).
      const listed = [await server.load('media'), await server.load('media.listMedia')];
      if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ['base-coherent'];
      base = { kind: 'illustration', id: payload.media };
    } else return ['base-coherent'];
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.base = base;
      diffusion.view = { z: payload.zoom ?? 1, px: payload.panX ?? 0, py: payload.panY ?? 0 };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
  // live.showCover (crates/live/data-capabilities/show-cover): the player
  // view goes back to the campaign's cover, with neither a media nor an
  // instance as its base (`base-coherent`); the floating window and the
  // framing stay as they are.
  'live.showCover': async () => {
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.base = { kind: 'cover' };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
  // live.showWindowOnPlayerView (crates/live/data-capabilities/show-window-on-player-view):
  // an illustration of the campaign opens in the floating window at its
  // default position and width, the three set together
  // (`window-fields-together`); its height follows the image's ratio and is
  // never stored (`window-proportional`). Another illustration replaces the
  // window's; the base and the framing stay as they are.
  'live.showWindowOnPlayerView': async (server, { payload }) => {
    const id = payload.media ?? payload.id;
    // A mock-up page that opened the window itself names the kind alone.
    if (!id) return payload.kind && payload.kind !== 'illustration' ? ['window-proportional'] : undefined;
    // An illustration listed, or one uploaded since (`media.uploadMedia`).
    const listed = [await server.load('media'), await server.load('media.listMedia')];
    if (!listed.some((media) => media && media.illustrations.some((i) => i.id === id))) return ['window-proportional'];
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.window = { id, x: 0.6, y: 0.12, w: 0.3 };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
  // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
  // added to the library. One with no name or no file is refused before
  // anything is queued, as the server refuses it. Its tags, a list of texts,
  // are normalised (`tags-normalized`); its own volume is a percentage from
  // 0 to 100 — a number or the text of one — full when none is given
  // (`sound-volume-range`).
  'soundbox.addSound': async (server, { payload }) => {
    const name = typeof payload.name === 'string' ? payload.name.trim() : '';
    const file = typeof payload.file === 'string' ? payload.file.trim() : '';
    if (!name || !file) return ['unprocessable'];
    const tags = payload.tags ?? [];
    if (!Array.isArray(tags) || !tags.every((t) => typeof t === 'string')) return ['tags-normalized'];
    const given = payload.volume ?? 100;
    const volume = typeof given === 'number' || (typeof given === 'string' && given.trim() !== '') ? Number(given) : NaN;
    if (!(volume >= 0 && volume <= 100)) return ['sound-volume-range'];
    const library = await server.load('soundbox.listSounds');
    library.sounds.unshift({ id: server.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
    return undefined;
  },
  // session.createSession (crates/session/data-capabilities/create-session):
  // the next session of the campaign, at the position after the last, in
  // preparation, with the PCs ticked present. `present-only-pj` refuses a
  // character that is not a PC of the fixtures; `oneshot-single-session` a
  // second session of a oneshot.
  'session.createSession': async (server, { payload }) => {
    const sessions = await server.load('session.listSessions');
    const characters = await server.load('campaign.listCharacters');
    const campaign = ((await server.load('campaign.listCampaigns')) || []).find((c) => c.id === payload.campaign);
    const present = Array.isArray(payload.present) ? payload.present : [];
    const pcs = new Set(characters.filter((c) => c.kind === 'pj').map((c) => c.id));
    if (present.some((id) => !pcs.has(id))) return ['present-only-pj'];
    if (campaign && campaign.type === 'oneshot' && (campaign.sessions ?? 0) > 0) return ['oneshot-single-session'];
    if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
    sessions.push({
      id: server.uuid(),
      order: sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1,
      title: String(payload.title || ''),
      date: payload.plannedOn || null,
      status: 'prep',
      label: 'En préparation',
      present,
      summary: null,
    });
    return undefined;
  },
  // session.addPresentCharacter (crates/session/data-capabilities/add-present-character):
  // one more PC present at the session — `targetId`, or `payload.session` —
  // listed in its `present` from then on. `present-only-pj` refuses a
  // character that is not a PC of the fixtures, which hold one campaign; a
  // session they do not list is `not-found`. A PC present already stays
  // present once.
  'session.addPresentCharacter': async (server, { payload, targetId }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === (targetId || payload.session));
    if (!session) return ['not-found'];
    const characters = await server.load('campaign.listCharacters');
    if (!characters.some((c) => c.kind === 'pj' && c.id === payload.character)) return ['present-only-pj'];
    if (!session.present.includes(payload.character)) session.present.push(payload.character);
    return undefined;
  },
  // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
  // the session's scenes in the order the DM gives, every rank rewritten at
  // once, 1 to n. An order that omits a scene of the fixtures' session,
  // repeats one or names one it does not hold is `scene-order-contiguous`,
  // and no rank changes.
  'session.reorderScenes': async (server, { payload }) => {
    const scenes = await server.load('session.listScenes');
    const order = Array.isArray(payload.order) ? payload.order : [];
    const known = new Set(scenes.map((sc) => sc.id));
    if (order.length !== known.size || new Set(order).size !== order.length || order.some((id) => !known.has(id))) return ['scene-order-contiguous'];
    for (const sc of scenes) sc.order = order.indexOf(sc.id) + 1;
    scenes.sort((a, b) => a.order - b.order);
    return undefined;
  },
  // session.removePresentCharacter (crates/session/data-capabilities/remove-present-character):
  // a PC no longer present at the session — `targetId`, or `payload.session`
  // — taken out of its `present` (which `session.getSession` reads too), and
  // so no longer offered (`present-only-pj`); the character itself stays in
  // the fixtures. A PC who is not present changes nothing; a session they do
  // not list is `not-found`.
  'session.removePresentCharacter': async (server, { payload, targetId }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === (targetId || payload.session));
    if (!session) return ['not-found'];
    session.present = session.present.filter((pc) => pc !== payload.character);
    return undefined;
  },
  // media.uploadMedia (crates/media/data-capabilities/upload-media): a
  // battlemap or an illustration added to the library, the path the DM chose
  // recorded and no file copied. A battlemap arrives with its default grid
  // (`grid-only-for-battlemap`) — a 32 px hex, pointy, 45 % opaque — at 5
  // feet per hex; an illustration has none. A place, when one is named, is a
  // lieu of the campaign (`media-place-is-lieu`); a kind that is neither is
  // refused (`media-kind`, the table's CHECK).
  'media.uploadMedia': async (server, { payload }) => {
    const { kind, place = null } = payload;
    if (kind !== 'battlemap' && kind !== 'illustration') return ['media-kind'];
    if (place && !((await server.load('campaign.listPlaces')) || []).some((p) => p.id === place)) return ['media-place-is-lieu'];
    const base = { id: server.uuid(), title: String(payload.title || '').trim() || 'Sans titre', file: payload.file || '', uploadedAt: new Date().toISOString().slice(0, 10), place };
    // The library is read as `media.listMedia`, the grid editor as `media`: the new medium is in both.
    for (const name of ['media', 'media.listMedia']) {
      const media = await server.load(name);
      if (!media) continue;
      if (kind === 'battlemap') media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: 'pointy', color: 'text', opacity: 45 } });
      else media.illustrations.push({ ...base, ratio: payload.width > 0 && payload.height > 0 ? Math.round((payload.width / payload.height) * 100) / 100 : null });
    }
    return undefined;
  },
  // media.saveGridSettings (crates/media/data-capabilities/save-grid-settings):
  // a battlemap's grid kept — the six fields of HexGrid@1, the opacity a
  // fraction of 1 where the mock-up's fixtures write a percentage. A hex size
  // of 0 or an opacity outside 0..1 is refused by `grid-values-range`; an
  // illustration or an unknown id has no grid (`grid-only-for-battlemap`).
  'media.saveGridSettings': async (server, { payload, targetId }) => {
    const battlemap = (await server.load('media')).battlemaps.find((b) => b.id === targetId);
    if (!battlemap) return ['grid-only-for-battlemap'];
    const { size, ox, oy, orient, color, opacity } = payload;
    if (!(size > 0) || !(opacity >= 0 && opacity <= 1)) return ['grid-values-range'];
    battlemap.grid = { size, ox, oy, orient, color, opacity: Math.round(opacity * 100) };
    return undefined;
  },
  // media.setMediaPlace (crates/media/data-capabilities/set-media-place): a
  // media — battlemap or illustration — attached to one of the campaign's
  // places, or detached with `place: null`, and nothing else of it changes. A
  // place that is no lieu the fixtures list is refused by
  // `media-place-is-lieu`; an unknown media is `not-found`.
  'media.setMediaPlace': async (server, { payload, targetId }) => {
    const id = targetId || payload.media;
    const listed = [await server.load('media'), await server.load('media.listMedia')];
    const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
    if (!found.length) return ['not-found'];
    const place = payload.place ?? null;
    if (place !== null && !(await server.load('campaign.listPlaces')).some((p) => p.id === place)) return ['media-place-is-lieu'];
    for (const medium of found) medium.place = place;
    return undefined;
  },
  // session.addScene (crates/session/data-capabilities/add-scene): a new
  // scene, ranked after the session's last (`scene-order-contiguous`), with
  // its title, its notes and the places it links — each a place of the
  // fixtures' campaign, else `scene-links-same-campaign` — whose media
  // follow it to the session (`session-media-by-place`, derived, never
  // stored). Its notes are kept as written. A blank title is
  // `scene-title-required`: the mock answers violation ids, where the
  // server answers 422.
  'session.addScene': async (server, { payload }) => {
    const title = String(payload.title || '').trim();
    if (!title) return ['scene-title-required'];
    const known = new Set((await server.load('campaign.listPlaces')).map((p) => p.id));
    const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
    if (places.some((id) => !known.has(id))) return ['scene-links-same-campaign'];
    const library = await server.load('media.listMedia');
    const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
    const scenes = await server.load('session.listScenes');
    scenes.push({
      id: server.uuid(),
      order: scenes.reduce((last, sc) => Math.max(last, sc.order), 0) + 1,
      title,
      places,
      characters: [],
      events: [],
      media,
      toCreate: [],
      battlemap: null,
      notes: typeof payload.notes === 'string' ? payload.notes : null,
    });
    return undefined;
  },
  // session.addPlaceToSession (crates/session/data-capabilities/add-place-to-session):
  // a place of the campaign joins a scene of the session — `payload.scene`,
  // else the first — after its last element, and stays there once (a place
  // already in the scene is not linked twice). Its media follow by
  // `session-media-by-place`: they are listed in the scene's media, never
  // stored apart. A place that is no lieu of the campaign is refused by
  // `scene-links-same-campaign`; a scene the session does not have is
  // `not-found` — the server answers 422 `scene-links-same-campaign` for a
  // scene of another session, which the fixtures, one session's, never list.
  'session.addPlaceToSession': async (server, { payload }) => {
    const place = payload.place;
    const scenes = (await server.load('session.listScenes')) || [];
    const first = scenes.reduce((low, sc) => (!low || sc.order < low.order ? sc : low), null);
    const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
    if (!scene) return ['not-found'];
    if (!((await server.load('campaign.listPlaces')) || []).some((p) => p.id === place)) return ['scene-links-same-campaign'];
    if (!scene.places.includes(place)) scene.places.push(place);
    const library = (await server.load('media.listMedia')) || { battlemaps: [], illustrations: [] };
    for (const medium of library.battlemaps.concat(library.illustrations)) {
      if (medium.place === place && !scene.media.includes(medium.id)) scene.media.push(medium.id);
    }
    return undefined;
  },
  // campaign.restoreEntityVersion (EntityVersion@1, restore-exact): the element `targetId` goes back to exactly
  // the version `payload.version` — or, with `payload.changes`, only the changes at those positions of the
  // version after it are undone, each field back to what it was. Either way the restoration is a new version
  // on top of the history — source restauration, restoredFrom the version — whose changes are what moved since
  // the version it replaces; versions 1..n stay as they were. A version the element does not have is refused.
  // table.createBattlemapInstance (crates/table/data-capabilities/create-battlemap-instance):
  // the scene's own copy of a battlemap of the campaign, empty; the battlemap, its grid and
  // the scene's media are left as they are (`instance-isolated`). A scene of another session,
  // or a battlemap the campaign's library does not list, is `not-found`; a scene that has its
  // instance is refused (`one-instance-per-scene`).
  'table.createBattlemapInstance': async (server, { payload }) => {
    const session = await server.load('session.getSession');
    const scene = (await server.load('session.listScenes')).find((sc) => sc.id === payload.scene);
    const map = (await server.load('media.listMedia')).battlemaps.find((m) => m.id === payload.battlemap);
    if (!session || session.id !== payload.session || !scene || !map) return ['not-found'];
    if (scene.battlemap) return ['one-instance-per-scene'];
    const id = `bmi-${map.id}-${scene.id}`;
    server.instances = server.instances || new Map();
    server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
    scene.battlemap = id;
    return undefined;
  },
  'campaign.restoreEntityVersion': async (server, { payload, targetId }) => {
    const history = (await server.load('campaign.listEntityVersions'))[targetId];
    const target = history && history.versions.find((v) => v.v === payload.version);
    if (!target) return ['not-found'];
    // The sheet as each version left it: fields by name, from the oldest version up.
    const sheet = (upTo) => history.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo)
      .reduce((fields, v) => { for (const c of v.changes) fields[c.field] = c.new; return fields; }, {});
    const before = sheet(history.versions[0].v);
    let after = sheet(target.v);
    if (Array.isArray(payload.changes)) {
      const undone = (history.versions.find((v) => v.v === payload.version + 1) || { changes: [] }).changes;
      after = { ...before };
      for (const i of payload.changes) if (undone[i]) after[undone[i].field] = undone[i].old;
    }
    const changes = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort()
      .filter((field) => (before[field] ?? '') !== (after[field] ?? ''))
      .map((field) => ({ field, old: before[field] ?? '', new: after[field] ?? '' }));
    history.versions.unshift({ v: history.versions[0].v + 1, date: new Date().toISOString(), origin: 'manuelle', source: 'restauration', restoredFrom: target.v, changes });
    return undefined;
  },
  // campaign.exportCampaign (crates/dataguard/data-capabilities/export-campaign):
  // stamps the campaign's last export with today's date and changes nothing
  // else (`export-stamps-date`). An unknown campaign is `not-found`.
  'campaign.exportCampaign': async (server, { payload }) => {
    const campaign = await server.load('campaign.getCampaign');
    if (!campaign || campaign.id !== payload.id) return ['not-found'];
    const now = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    campaign.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
    return undefined;
  },
  // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
  // name, tags and volume of one sound, tags normalised and volume ranged as
  // for an addition; a sound that is not there is `not-found`.
  'soundbox.updateSound': async (server, { payload, targetId }) => {
    const sound = (await server.load('soundbox.listSounds')).sounds.find((s) => s.id === targetId);
    if (!sound) return ['not-found'];
    if ('volume' in payload && !(Number(payload.volume) >= 0 && Number(payload.volume) <= 100)) return ['sound-volume-range'];
    if ('name' in payload) sound.name = String(payload.name);
    if ('tags' in payload) sound.tags = soundTags(payload.tags);
    if ('volume' in payload) sound.volume = Number(payload.volume);
    if ('file' in payload) sound.file = String(payload.file);
    return undefined;
  },
  // soundbox.removeSound (crates/soundbox/data-capabilities/remove-sound): the
  // sound leaves the library's readers (a soft delete, its tombstone is the
  // server's).
  'soundbox.removeSound': async (server, { targetId }) => {
    const library = await server.load('soundbox.listSounds');
    const at = library.sounds.findIndex((s) => s.id === targetId);
    if (at < 0) return ['not-found'];
    library.sounds.splice(at, 1);
    return undefined;
  },
  // soundbox.setMasterVolume (crates/soundbox/data-capabilities/set-master-volume):
  // the master slider, in percent, kept to the whole percent; every sound
  // follows it, keeping their ratio (EffectiveVolume@1). A volume outside
  // 0..100, or no number, is refused by `master-volume-range`, never clamped;
  // a request naming no campaign is `not-found`.
  'soundbox.setMasterVolume': async (server, { payload, targetId }) => {
    if (!String(targetId || payload.campaign || '').trim()) return ['not-found'];
    const { volume } = payload;
    if (typeof volume !== 'number' || !(volume >= 0 && volume <= 100)) return ['master-volume-range'];
    for (const name of ['soundbox.listSounds', 'sounds']) {
      const fixture = await server.load(name);
      if (fixture) fixture.masterVolume = Math.round(volume);
    }
    return undefined;
  },
  // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
  // running playback gets its stop (`stop-all`); with none running it still
  // succeeds and changes nothing.
  'soundbox.stopAll': async (server) => {
    for (const sound of (await server.load('soundbox.listSounds')).sounds) sound.playing = false;
    return undefined;
  },
  // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
  // one sound's own volume, in percent, from 0 to 100; out of range it is
  // refused by `sound-volume-range`, never clamped. Nothing else of the sound
  // changes; its effective gain follows (EffectiveVolume@1).
  'soundbox.setSoundVolume': async (server, { payload, targetId }) => {
    const id = targetId || payload?.sound;
    const fixtures = (await Promise.all(['soundbox.listSounds', 'sounds'].map((name) => server.load(name)))).filter(Boolean);
    const found = fixtures.map((fixture) => fixture.sounds.find((candidate) => candidate.id === id)).filter(Boolean);
    if (!found.length) return ['not-found'];
    const volume = payload?.volume;
    if (typeof volume !== 'number' || !(volume >= 0 && volume <= 100)) return ['sound-volume-range'];
    for (const sound of found) sound.volume = volume;
    return undefined;
  },
  // campaign.createCampaign (crates/campaign/data-capabilities/create-campaign):
  // a new campaign at the top of the home page, its master volume at full
  // (`master-volume-range`); a oneshot comes with its single session, in
  // preparation (`oneshot-single-session`), a campagne with none. A campaign
  // with no name is refused, as one of a kind there is not.
  'campaign.createCampaign': async (server, { payload }) => {
    const name = String(payload.name ?? '').trim();
    if (!name) return ['campaign-name-required'];
    if (!['campagne', 'oneshot'].includes(payload.type)) return ['campaign-type-unknown'];
    const text = (field) => (typeof payload[field] === 'string' ? payload[field] : null);
    (await server.load('campaign.listCampaigns')).unshift({
      id: server.uuid(),
      name,
      type: payload.type,
      art: text('art'),
      glyph: text('glyph'),
      pitch: text('pitch'),
      sessions: payload.type === 'oneshot' ? 1 : 0,
      lastPlayed: null,
      next: null,
    });
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
  // ai.extractFromNotes (crates/ai/data-capabilities/extract-from-notes): the
  // note analysed into proposals, nothing created (`no-creation-before-
  // validation`). The mock's AI proposes what the fixture NoteImport lists; a
  // proposal that names an existing element starts unchecked.
  'ai.extractFromNotes': async (server, { payload }) => {
    const text = String(payload.text || '');
    const { proposals } = await server.load('NoteImport');
    server.noteText = text;
    server.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
    return undefined;
  },
  // ai.analyzeBrief (crates/ai/data-capabilities/analyze-brief): the brief told
  // or typed, kept as text (`text-only-no-audio`), cut into scenes with the
  // elements recognised and the ones to create — nothing created
  // (`brief-no-auto-creation`). The mock's AI finds in any brief what the
  // fixture OralBrief lists; the brief becomes session 4's last, and the
  // existing NPC it changes gets a version, as the real analysis writes one.
  'ai.analyzeBrief': async (server, { payload }) => {
    const text = String(payload.text || '').trim();
    const brief = await server.load('session.getLastBrief');
    const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
    Object.assign(brief, { transcript: lines, lastRun: new Date().toISOString().slice(0, 19) });
    const histories = await server.load('campaign.listEntityVersions');
    const history = histories.morlox;
    const change = (await server.load('OralBrief')).result.recognized.find((x) => x.updated);
    if (history && change) {
      const latest = history.versions[0];
      history.versions.unshift({ v: latest ? latest.v + 1 : 1, date: brief.lastRun, origin: 'ia', source: `Brief oral — séance ${String(payload.session).replace(/^s/, '')}`, restoredFrom: null, seen: false, changes: latest ? latest.changes : [] });
    }
    return undefined;
  },
  // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
  // one element per accepted proposal, the raw note kept as an import listed
  // newest first, the proposals no longer awaiting.
  'campaign.createEntitiesFromNote': async (server, { payload }) => {
    const proposals = server.noteProposals || [];
    const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
    const created = proposals.filter((p) => accepted.includes(p.id));
    const fixture = await server.load('campaign.listImportedNotes');
    const imports = Array.isArray(fixture) ? fixture : fixture.imports;
    const note = server.noteText || '';
    imports.unshift({ id: `imp-${server.uuid()}`, date: new Date().toISOString().slice(0, 10), title: note.split('\n')[0].replace(/[.\s]+$/, '') || 'Notes importées', chars: note.length, proposed: proposals.length, created: created.length });
    server.noteProposals = [];
    return undefined;
  },
  // ai.createFromBrief (crates/ai/data-capabilities/create-from-brief): the
  // to-create entry becomes an element of the campaign, from its excerpt alone
  // or from the excerpt and the DM's refinement (the fixture ToCreateList
  // gives the AI's proposal for each); the entry is created once its element
  // is set (`to-create-leaves-once-created`), and an entry already created is
  // refused. An NPC gets its background, behaviour and sheet.
  'ai.createFromBrief': async (server, { payload }) => {
    const entries = await server.load('campaign.listEntitiesToCreate');
    const entry = (Array.isArray(entries) ? entries : entries?.items || []).find((candidate) => candidate.id === payload.entry);
    if (!entry) return ['not-found'];
    if (entry.status === 'created') return ['to-create-leaves-once-created'];
    const given = (await server.load('ToCreateList')).proposals?.[entry.id] || {};
    const refined = typeof payload.refinement === 'string' && payload.refinement.trim() !== '';
    const proposal = (refined ? given.refined : given.fromBrief) || { description: entry.excerpt };
    const { statblock, ...text } = proposal;
    const kind = { pnj: 'pnj', lieu: 'lieu', 'événement': 'evenement' }[entry.type];
    const entities = await server.load('entities');
    const list = { pnj: entities.characters, lieu: entities.places, evenement: entities.events }[kind];
    const id = `${entry.id.replace(/^tc-/, '')}-${server.uuid().slice(0, 8)}`;
    const figure = (value) => (Number.isNaN(parseInt(value, 10)) ? null : parseInt(value, 10));
    const vital = (label) => statblock?.vitals?.find((v) => v.label === label)?.value;
    list.push({
      id, name: entry.name, aliases: [], kind, notes: '', ...text,
      ...(statblock ? { type: statblock.type, stats: statblock.abilities, ca: figure(vital('CA')), pvMax: figure(vital('PV')), speed: vital('Vitesse'), features: statblock.features } : {}),
    });
    entry.status = 'created';
    entry.entity = id;
    return undefined;
  },
  // campaign.saveEntity (crates/dataguard/data-capabilities/save-entity): the
  // DM's explicit save of an element, its sheet and its aliases, which adds a
  // version (EntityVersion@1, source `dm`) whose changes are the fields that
  // moved. An alias naming another element, by its name or an alias
  // (AliasMatch@1), is refused by `alias-unique-in-campaign`; a player on
  // anything but a PC by `character-is-pj-or-pnj`. Only the editor's fields
  // are kept: a DEX modifier given is never written.
  'campaign.saveEntity': async (server, { payload }) => {
    const given = payload.entity || payload;
    const lists = await Promise.all(Object.values(LISTS).filter((v, i, all) => all.indexOf(v) === i).map(async (name) => [name, (await server.load(name)) || []]));
    const all = lists.flatMap(([name, list]) => list.map((element) => ({ name, element })));
    const found = all.find(({ element }) => given.id && element.id === given.id);
    const before = found ? found.element : {};
    const kind = given.kind || before.kind || (found && found.name === 'campaign.listPlaces' ? 'lieu' : found && found.name === 'campaign.listEvents' ? 'evenement' : undefined);
    if (!LISTS[kind]) return ['character-is-pj-or-pnj'];
    const after = { ...before };
    for (const field of SAVED) if (field in given) after[field] = given[field];
    after.kind = kind;
    if (Array.isArray(after.aliases)) {
      const seen = new Set();
      after.aliases = after.aliases.filter((a) => typeof a === 'string' && normalized(a) && !seen.has(normalized(a)) && seen.add(normalized(a)));
    }
    if (after.player && kind !== 'pj') return ['character-is-pj-or-pnj'];
    const own = [[normalized(after.name), false], ...(after.aliases || []).map((a) => [normalized(a), true])].filter(([t]) => t);
    const others = all.filter(({ element }) => element !== before)
      .flatMap(({ element }) => [[normalized(element.name), false], ...(element.aliases || []).map((a) => [normalized(a), true])]);
    if (own.some(([t, alias]) => others.some(([o, otherAlias]) => o === t && (alias || otherAlias)))) return ['alias-unique-in-campaign'];
    if ((kind === 'pj' || kind === 'pnj') && !after.color) after.color = 'accent';
    const id = before.id || given.id || server.uuid();
    after.id = id;
    const list = (await server.load(LISTS[kind])) || [];
    if (found && found.name !== LISTS[kind]) {
      const old = lists.find(([name]) => name === found.name)[1];
      old.splice(old.indexOf(before), 1);
    }
    const at = list.indexOf(before);
    if (at >= 0) list[at] = after; else list.push(after);
    const history = (await server.load('campaign.listEntityVersions')) || {};
    const entry = history[id] || (history[id] = { name: after.name, versions: [] });
    entry.name = after.name;
    const changes = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== 'id').sort()
      .filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null))
      .map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
    const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
    entry.versions.unshift({ v, date: new Date().toISOString().slice(0, 19), origin: 'dm', source: 'Fiche enregistrée par le DM', seen: true, changes });
    return undefined;
  },
  // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
  // playback of the sound starts, whatever else plays (`simultaneous-playback`),
  // from the soundbox (the default) or a suggestion's pastille; any other
  // source, the player view's, is refused (`dm-side-only`). The sound plays
  // as soundbox.listSounds says it: `playing`.
  'soundbox.playSound': async (server, { payload, targetId }) => {
    const id = targetId || payload?.sound;
    const sound = (await server.load('soundbox.listSounds')).sounds.find((candidate) => candidate.id === id);
    if (!sound) return ['not-found'];
    const source = payload?.source ?? 'soundbox';
    if (!['soundbox', 'suggestion'].includes(source)) return ['dm-side-only'];
    sound.playing = true;
    return undefined;
  },
};
