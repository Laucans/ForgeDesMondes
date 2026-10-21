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
  // table.setTokenCondition: the badges of that token alone.
  'table.setTokenCondition': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.states = [...(payload.conditions || [])]; }),
  // table.removeToken: the token leaves the map.
  'table.removeToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t, state) => { state.tokens = state.tokens.filter((x) => x !== t); }),
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
    const media = await server.load('media');
    const base = { id: server.uuid(), title: String(payload.title || '').trim() || 'Sans titre', file: payload.file || '', uploadedAt: new Date().toISOString().slice(0, 10), place };
    if (kind === 'battlemap') media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: 'pointy', color: 'text', opacity: 45 } });
    else media.illustrations.push(base);
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
  // campaign.restoreEntityVersion (EntityVersion@1, restore-exact): the element `targetId` goes back to exactly
  // the version `payload.version` — or, with `payload.changes`, only the changes at those positions of the
  // version after it are undone, each field back to what it was. Either way the restoration is a new version
  // on top of the history — source restauration, restoredFrom the version — whose changes are what moved since
  // the version it replaces; versions 1..n stay as they were. A version the element does not have is refused.
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
};
