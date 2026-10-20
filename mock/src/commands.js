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

export const COMMANDS = {
  // session.addPresentCharacter / session.removePresentCharacter (the Session
  // aggregate): a PC of the session's campaign ticked present or unticked.
  // `present-only-pj`: a character that is not a PC of the campaign is refused.
  'session.addPresentCharacter': async (server, { payload }) => {
    const session = await server.load('session.getSession');
    const character = (await server.load('campaign.listCharacters')).find((c) => c.id === payload.character);
    if (!character || character.kind !== 'pj') return ['present-only-pj'];
    if (!session.present.includes(character.id)) session.present.push(character.id);
    return undefined;
  },
  'session.removePresentCharacter': async (server, { payload }) => {
    const session = await server.load('session.getSession');
    session.present = session.present.filter((id) => id !== payload.character);
    return undefined;
  },
  // session.addScene: a scene at the end, its rank the next one.
  'session.addScene': async (server, { payload }) => {
    const scenes = await server.load('session.listScenes');
    const next = scenes.length + 1;
    scenes.push({ id: `sc${Date.now().toString(36)}`, order: next, title: String(payload.title || 'Nouvelle scène'), places: [], characters: [], events: [], media: [], toCreate: [], battlemap: null, notes: '' });
    return undefined;
  },
  // session.reorderScenes: every rank rewritten at once. `scene-order-contiguous`:
  // the ranks must be 1 to n, each scene once.
  'session.reorderScenes': async (server, { payload }) => {
    const scenes = await server.load('session.listScenes');
    const order = Array.isArray(payload.order) ? payload.order : [];
    const ranks = order.map((o) => o.rank).sort((a, b) => a - b);
    const known = new Set(scenes.map((sc) => sc.id));
    if (order.length !== scenes.length || ranks.some((rank, i) => rank !== i + 1) || order.some((o) => !known.has(o.scene)) || new Set(order.map((o) => o.scene)).size !== scenes.length) return ['scene-order-contiguous'];
    for (const o of order) scenes.find((sc) => sc.id === o.scene).order = o.rank;
    scenes.sort((a, b) => a.order - b.order);
    return undefined;
  },
  // table.createBattlemapInstance: the scene's own copy of a battlemap of the
  // campaign, empty. `one-instance-per-scene`: a scene that has one is refused.
  'table.createBattlemapInstance': async (server, { payload }) => {
    const scene = (await server.load('session.listScenes')).find((sc) => sc.id === payload.scene);
    const map = (await server.load('media.listMedia')).battlemaps.find((m) => m.id === payload.battlemap);
    if (!scene || !map) return ['not-found'];
    if (scene.battlemap) return ['one-instance-per-scene'];
    const id = `bmi-${map.id}-${scene.id}`;
    server.instances = server.instances || new Map();
    server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
    scene.battlemap = id;
    if (!scene.media.includes(map.id)) scene.media.push(map.id);
    return undefined;
  },
  // table.placeTokens: a character dropped on an instance at a hex, « × count » (1 to 12). An NPC's tokens are
  // numbered after the highest number it holds there and named with it (`multi-drop-numbering`), each with
  // the sheet's HP max as its HP (`pj-hp-not-tracked`); a PC is dropped once, only when present at the
  // session, and only when it has no token there yet (`token-roster`).
  'table.placeTokens': async (server, { payload, targetId }) => {
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const inst = id === fixture.id ? fixture : server.instances && server.instances.get(id);
    const character = (await server.load('campaign.listCharacters')).find((c) => c.id === payload.character);
    if (!inst || !character) return ['not-found'];
    const count = payload.count ?? 1;
    if (!Number.isInteger(count) || count < 1 || count > 12) return ['multi-drop-numbering'];
    if (!Number.isInteger(payload.q) || !Number.isInteger(payload.r)) return ['single-position-source'];
    const tokens = inst.state.tokens;
    const mine = tokens.filter((t) => t.char === character.id);
    const pc = character.kind === 'pj';
    if (pc && (count !== 1 || mine.length || !(await server.load('session.getSession')).present.includes(character.id))) return ['token-roster'];
    const hp = pc ? null : character.pvMax ?? null;
    let num = Math.max(0, ...mine.map((t) => t.num || 0));
    for (let i = 0; i < count; i++) {
      num += pc ? 0 : 1;
      tokens.push({ id: server.uuid(), char: character.id, kind: character.kind, name: pc ? character.name : `${character.name} ${num}`, num: pc ? null : num, q: payload.q, r: payload.r, visible: true, hp, hpMax: hp, states: [], dead: false });
    }
    return undefined;
  },
  // table.setTokenVisibility: one token hidden from the players or revealed, `payload.token` on the instance
  // `targetId` — or the token itself as the target. Its visibility alone: its initiative rank and every other
  // participant's stay as they are (`hidden-keeps-rank`); a visibility that is no boolean is refused.
  'table.setTokenVisibility': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    if (typeof payload.visible !== 'boolean') return ['players-see-band-only'];
    // A `token` sent, even null, names the token: the target is then its instance, as the server reads it.
    const named = payload.token !== undefined;
    const tokenId = named ? payload.token : targetId;
    if (typeof tokenId !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!inst) return ['not-found'];
    inst.state.tokens.find((t) => same(t.id, tokenId)).visible = payload.visible;
    return undefined;
  },
  // table.changeHitPoints: one NPC token's current HP changed by `payload.delta`, `payload.token` on the
  // instance `targetId` — or the token itself as the target. Its HP land between 0 and its HP max, and at 0
  // it is dead, derived from its HP alone (`dead-derived`); the other tokens of its character are untouched
  // (`hp-change-one-token`). A PC's token tracks no HP, and a change that is no whole number is refused.
  'table.changeHitPoints': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    if (!Number.isInteger(payload.delta)) return ['pj-hp-not-tracked'];
    const named = payload.token !== undefined;
    const tokenId = named ? payload.token : targetId;
    if (typeof tokenId !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!inst) return ['not-found'];
    const token = inst.state.tokens.find((t) => same(t.id, tokenId));
    if (!Number.isInteger(token.hp) || !Number.isInteger(token.hpMax)) return ['pj-hp-not-tracked'];
    token.hp = Math.max(0, Math.min(token.hpMax, token.hp + payload.delta));
    token.dead = token.hp === 0;
    return undefined;
  },
  // table.renameToken: one token renamed, `payload.token` on the instance `targetId` — or the token itself as
  // the target, or no target. Its number and the other tokens of its character are untouched
  // (`multi-drop-numbering`); a name of blanks alone is refused. Ids are compared in any case, as the server does.
  'table.renameToken': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const name = typeof payload.name === 'string' ? payload.name.trim() : '';
    if (!name) return ['multi-drop-numbering'];
    const tokenId = payload.token ?? targetId;
    const instance = payload.token != null && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!inst) return ['not-found'];
    inst.state.tokens.find((t) => same(t.id, tokenId)).name = name;
    return undefined;
  },
  // table.startCombat: the combat of the instance `targetId` — or `payload.instance` —, every token in it, a
  // hidden one like any other (`hidden-keeps-rank`). An NPC rolls `payload.rolls[token]` or a d20, plus its DEX
  // modifier; a PC rolls nothing and takes the initiative `payload.initiative[character]` the DM entered
  // (`npc-initiative-bounds`). Ranked by initiative, then DEX score, then a random draw (`initiative-tiebreak`);
  // round 1 on rank 1. An instance whose combat is open starts none (`one-active-combat`); a PC with no
  // initiative, or a roll out of 1 to 20 or of no NPC token, is refused. Ids are compared in any case.
  'table.startCombat': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => same(i.id, id));
    if (!inst || !inst.state.tokens.length) return ['not-found'];
    if (inst.state.combat) return ['one-active-combat'];
    const figure = (figures, key) => Object.entries(figures || {}).find(([k]) => same(k, key))?.[1];
    const tokens = inst.state.tokens;
    if (Object.keys(payload.rolls || {}).some((k) => !tokens.some((t) => same(t.id, k) && t.kind === 'pnj'))) return ['npc-initiative-bounds'];
    const characters = await server.load('campaign.listCharacters');
    const seated = [];
    for (const t of tokens) {
      if (t.kind !== 'pj' && t.kind !== 'pnj') return ['not-found'];
      const score = characters.find((c) => same(c.id, t.char))?.stats?.DEX;
      const pc = t.kind === 'pj';
      const dexScore = pc ? score ?? null : score ?? 10;
      const roll = pc ? null : figure(payload.rolls, t.id) ?? 1 + Math.floor(Math.random() * 20);
      const init = pc ? figure(payload.initiative, t.char) : roll + Math.floor((dexScore - 10) / 2);
      if (!Number.isInteger(init) || (roll !== null && (!Number.isInteger(roll) || roll < 1 || roll > 20))) return ['npc-initiative-bounds'];
      seated.push({ id: t.id, init, roll, dexScore, draw: Math.random() });
    }
    // A stable sort: an unknown DEX after every known one, a full tie in the order given.
    const order = seated.sort((a, b) => (b.init - a.init) || ((b.dexScore ?? -Infinity) - (a.dexScore ?? -Infinity)) || (b.draw - a.draw))
      .map(({ id: token, init, roll, dexScore }) => ({ id: token, init, roll, dex: dexScore === null ? 0 : Math.floor((dexScore - 10) / 2) }));
    inst.state.combat = { round: 1, current: order[0].id, order };
    return undefined;
  },
  // table.endCombat: the open combat of the instance `targetId` — or `payload.instance` — ends: the carousel
  // goes, no turn is current, and the tokens, their HP and their badges stay as they are. An instance with no
  // combat open — or no such instance, which the server does not tell apart — ends none (`one-active-combat`);
  // a new combat may start once it ended. Ids are compared in any case, as the server does.
  'table.endCombat': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => same(i.id, id));
    if (!inst || !inst.state.combat) return ['one-active-combat'];
    inst.state.combat = null;
    return undefined;
  },
  // table.resetBattlemap: « Réinitialiser la carte » — the instance `targetId` — or `payload.instance` — loses
  // every token, its combat and their initiative; the instance itself, its battlemap and every other instance
  // stay as they are (`instance-isolated`). No such instance resets none, and one with no token and no combat
  // has nothing to reset, as the server refuses both. Ids are compared in any case, as the server does.
  'table.resetBattlemap': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => same(i.id, id));
    if (!inst) return ['not-found'];
    if (!inst.state.tokens.length && !inst.state.combat) return ['state-persisted-reset'];
    inst.state.tokens = [];
    inst.state.combat = null;
    return undefined;
  },
  // table.setTokenCondition: the badge `payload.state` put on (`on: true`) or taken off (`on: false`) one token,
  // `payload.token` on the instance `targetId` — or the token itself as the target, or no target. The other
  // tokens of its character keep their badges (`instance-isolated`); a badge put on twice is there once, and one
  // of blanks alone, or an `on` that is no boolean, is refused. Ids are compared in any case, as the server does.
  'table.setTokenCondition': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const state = typeof payload.state === 'string' ? payload.state.trim() : '';
    if (!state || state.includes('\0') || typeof payload.on !== 'boolean') return ['state-persisted-reset'];
    const tokenId = payload.token ?? targetId;
    const instance = payload.token != null && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!inst) return ['not-found'];
    const token = inst.state.tokens.find((t) => same(t.id, tokenId));
    const states = token.states || [];
    token.states = payload.on ? (states.includes(state) ? states : states.concat(state)) : states.filter((s) => s !== state);
    return undefined;
  },
  // table.reorderInitiative: the open combat of the instance `targetId` — or `payload.instance` — takes the order
  // `payload.order`, every participant once, named by its token: the first gets rank 1, the next 2, without gap
  // (`initiative-tiebreak`). A hidden one takes the rank it is given (`hidden-keeps-rank`); no participant is added
  // or removed and the turn does not move. An order that omits, repeats or adds a participant is refused, and so is
  // an instance with no open combat. Ids are compared in any case, as the server does.
  'table.reorderInitiative': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const combat = instances.find((i) => same(i.id, id))?.state.combat;
    if (!combat) return ['not-found'];
    const order = Array.isArray(payload.order) ? payload.order : [];
    const placed = order.map((named) => combat.order.findIndex((p) => same(p.id, named)));
    if (order.length !== combat.order.length || placed.some((at, i) => at < 0 || placed.indexOf(at) !== i)) return ['initiative-tiebreak'];
    combat.order = placed.map((at) => combat.order[at]);
    return undefined;
  },
  // table.removeParticipant: one participant taken out of the open combat, `payload.entry` in a combat of the
  // instance `targetId` — or the entry itself as the target, or no target. The mock-up's order names each
  // participant by its token, which stands for its entry here. Its entry's partition is tombstoned and nothing else
  // is written: its token stays on the map, the order's places close up, and a turn still on it is no one's — the
  // Battlemap sends table.nextTurn first when it holds the turn. Ids are compared in any case, as the server does.
  'table.removeParticipant': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    // An `entry` sent, even null, names the entry: the target is then its instance, as the server reads it.
    const named = payload.entry !== undefined;
    const entryId = named ? payload.entry : targetId;
    if (typeof entryId !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, entryId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const of = (o) => o.entry ?? o.id;
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.combat && i.state.combat.order.some((o) => same(of(o), entryId)));
    if (!inst) return ['not-found'];
    const combat = inst.state.combat;
    const gone = combat.order.find((o) => same(of(o), entryId));
    combat.order = combat.order.filter((o) => o !== gone);
    if (same(combat.current, gone.id)) combat.current = null;
    return undefined;
  },
  // table.nextTurn (Initiative@1, « Suivant »): the open combat of the instance `targetId` — or `payload.instance`
  // — passes the turn to the next living participant after the current one in its order, a hidden one like any
  // other (`hidden-keeps-rank`), a dead one skipped (`dead-derived`); the round goes up when the search wraps
  // past the last (`next-turn`). An instance with no open combat, or none of whose participants is alive, is
  // refused. Ids are compared in any case, as the server does.
  'table.nextTurn': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => same(i.id, id));
    const combat = inst?.state.combat;
    if (!combat) return ['next-turn'];
    const alive = combat.order.map((p) => inst.state.tokens.some((t) => same(t.id, p.id) && !t.dead));
    const at = combat.order.findIndex((p) => same(p.id, combat.current));
    const after = alive.findIndex((living, i) => living && i > at);
    const wrapped = after === -1 ? alive.findIndex((living, i) => living && i <= at) : -1;
    if (after === -1 && wrapped === -1) return ['next-turn'];
    combat.current = combat.order[after === -1 ? wrapped : after].id;
    if (after === -1) combat.round += 1;
    return undefined;
  },
  // table.removeToken: one token taken off the map, `payload.token` on the instance `targetId` — or the token
  // itself as the target, or no target. Its partition is tombstoned and the tables follow the aggregate's
  // relations: its entry leaves the order, whose places close up, and a turn still on it is no one's — the
  // Battlemap sends table.nextTurn first when it holds the turn. Ids are compared in any case, as the server does.
  'table.removeToken': async (server, { payload, targetId }) => {
    const same = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();
    // A `token` sent, even null, names the token: the target is then its instance, as the server reads it.
    const named = payload.token !== undefined;
    const tokenId = named ? payload.token : targetId;
    if (typeof tokenId !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const inst = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!inst) return ['not-found'];
    const id = inst.state.tokens.find((t) => same(t.id, tokenId)).id;
    inst.state.tokens = inst.state.tokens.filter((t) => t.id !== id);
    const combat = inst.state.combat;
    if (combat) {
      combat.order = combat.order.filter((o) => o.id !== id);
      if (combat.current === id) combat.current = null;
    }
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
