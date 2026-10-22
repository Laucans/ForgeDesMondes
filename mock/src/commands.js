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
    const next = Math.max(0, ...scenes.map((sc) => sc.order)) + 1;
    scenes.push({ id: `sc-${server.uuid().slice(0, 8)}`, order: next, title: String(payload.title || 'Nouvelle scène'), places: [], characters: [], events: [], media: [], toCreate: [], battlemap: null, notes: '' });
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
  // table.placeTokens (crates/table/data-capabilities/place-tokens): a
  // character dropped on the instance `targetId` (or `payload.instance`) at a
  // hex, `count` times (1 to 12, 1 by default). An NPC's tokens are numbered
  // after the highest its tokens hold (`multi-drop-numbering`), named « Bandit
  // 4 », and copy the sheet's HP max as their HP; a PC is dropped once, only
  // when present at the session, with no number and no HP (`token-roster`,
  // `pj-hp-not-tracked`). Every token stands on the hex, visible, with no
  // condition. An unknown instance or character is `not-found`; a hex that is
  // no pair of whole numbers is refused by `single-position-source`.
  'table.placeTokens': async (server, { payload, targetId }) => {
    const count = payload.count ?? 1;
    if (!Number.isInteger(payload.q) || !Number.isInteger(payload.r)) return ['single-position-source'];
    if (!Number.isInteger(count) || count < 1 || count > 12) return ['multi-drop-numbering'];
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instance = id === fixture.id ? fixture : server.instances && server.instances.get(id);
    const character = (await server.load('campaign.listCharacters')).find((c) => c.id === payload.character);
    if (!instance || !character || !['pj', 'pnj'].includes(character.kind)) return ['not-found'];
    const tokens = instance.state.tokens;
    const placed = tokens.filter((t) => t.char === character.id);
    const pnj = character.kind === 'pnj';
    if (!pnj) {
      const session = await server.load('session.getSession');
      if (count !== 1 || placed.length || !session.present.includes(character.id)) return ['token-roster'];
    }
    let number = Math.max(0, ...placed.map((t) => t.num || 0));
    for (let i = 0; i < count; i++) {
      const num = pnj ? ++number : null;
      tokens.push({
        id: server.uuid(), char: character.id, kind: character.kind, name: pnj ? `${character.name.trim()} ${num}` : character.name.trim(), num,
        q: payload.q, r: payload.r, visible: true, hp: pnj ? character.pvMax ?? null : null, hpMax: pnj ? character.pvMax ?? null : null, states: [], dead: false,
      });
    }
    if (!instance.roster.includes(character.id)) instance.roster.push(character.id);
    return undefined;
  },
  // table.renameToken (crates/table/data-capabilities/rename-token): one token
  // renamed, `payload.token` on the instance `targetId` — or the token itself
  // as the target. Its number and the other tokens of its character are
  // untouched (`multi-drop-numbering`); a name of blanks alone is refused. Ids
  // are compared in any case, as the server does.
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
  // table.moveToken (crates/table/data-capabilities/move-token): the token
  // `payload.token` of the instance `targetId` (or `payload.instance`) dropped
  // on the hex (q, r) — its position alone, the one both views read
  // (`single-position-source`); no other token, nor the battlemap and its
  // grid, moves. A hex that is no pair of whole numbers an `integer` holds is
  // refused by `single-position-source`; an unknown token is `not-found`; a
  // token that stands on another instance than the one named — or on none
  // named — is refused by `instance-isolated`, as the DataGuard judges it.
  'table.moveToken': async (server, { payload, targetId }) => {
    const axis = (n) => Number.isInteger(n) && n >= -(2 ** 31) && n < 2 ** 31;
    if (!axis(payload.q) || !axis(payload.r)) return ['single-position-source'];
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => i.state.tokens.some((t) => t.id === payload.token));
    if (!holder) return ['not-found'];
    if (holder.id !== id) return ['instance-isolated'];
    const token = holder.state.tokens.find((t) => t.id === payload.token);
    token.q = payload.q;
    token.r = payload.r;
    return undefined;
  },
  // table.setTokenVisibility (crates/table/data-capabilities/set-token-visibility):
  // the token `payload.token` of the instance `targetId` (or
  // `payload.instance`) hidden from the players or revealed — its `visible`
  // alone; its initiative rank and every other stay as they are
  // (`hidden-keeps-rank`). A visibility that is no boolean is refused by
  // `players-see-band-only`; an unknown token is `not-found`; a token that
  // stands on another instance than the one named — or on none named — is
  // refused by `instance-isolated`, as the DataGuard judges it. Ids are
  // compared in any case and without the blanks around them, as the server
  // does.
  'table.setTokenVisibility': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    if (typeof payload.visible !== 'boolean') return ['players-see-band-only'];
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => i.state.tokens.some((t) => same(t.id, payload.token)));
    if (!holder) return ['not-found'];
    if (!same(id, holder.id)) return ['instance-isolated'];
    holder.state.tokens.find((t) => same(t.id, payload.token)).visible = payload.visible;
    return undefined;
  },
  // table.setTokenCondition (crates/table/data-capabilities/set-token-condition):
  // the badge `payload.state` put on the token `payload.token` of the
  // instance `targetId` (or `payload.instance`) when `payload.on`, after the
  // others and once, or taken off, the others keeping their order — its
  // conditions alone; its HP, its death and the other tokens of the same
  // character stay as they are. A condition that is no name without the
  // blanks around it, or holds U+0000, or an `on` that is no boolean, is
  // refused by `state-persisted-reset`; an unknown token is `not-found`; a
  // token that stands on another instance than the one named — or on none
  // named — is refused by `instance-isolated`, as the DataGuard judges it.
  'table.setTokenCondition': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const state = typeof payload.state === 'string' ? payload.state.trim() : '';
    if (!state || state.includes('\0') || typeof payload.on !== 'boolean') return ['state-persisted-reset'];
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => i.state.tokens.some((t) => same(t.id, payload.token)));
    if (!holder) return ['not-found'];
    if (!same(id, holder.id)) return ['instance-isolated'];
    const token = holder.state.tokens.find((t) => same(t.id, payload.token));
    const held = token.states || [];
    token.states = payload.on ? (held.includes(state) ? held : [...held, state]) : held.filter((s) => s !== state);
    return undefined;
  },
  // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
  // the NPC token `payload.token` of the instance `targetId` (or
  // `payload.instance`) wounded or healed by `payload.delta` — its HP alone,
  // landing between 0 and its HP max (`pj-hp-not-tracked`); the other tokens
  // of its character are untouched (`hp-change-one-token`). At 0 it is dead,
  // derived from its HP alone (`dead-derived`). A change that is no whole
  // number, or one on a PC's token, which tracks no HP, is refused by
  // `pj-hp-not-tracked`; an unknown token is `not-found`; a token that stands
  // on another instance than the one named — or on none named — is refused
  // by `instance-isolated`, as the DataGuard judges it. Ids are compared in
  // any case and without the blanks around them, as the server does.
  'table.changeHitPoints': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    if (!Number.isInteger(payload.delta)) return ['pj-hp-not-tracked'];
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => i.state.tokens.some((t) => same(t.id, payload.token)));
    if (!holder) return ['not-found'];
    if (!same(id, holder.id)) return ['instance-isolated'];
    const token = holder.state.tokens.find((t) => same(t.id, payload.token));
    if (!Number.isInteger(token.hp) || !Number.isInteger(token.hpMax)) return ['pj-hp-not-tracked'];
    token.hp = Math.max(0, Math.min(token.hpMax, token.hp + payload.delta));
    token.dead = token.hp === 0;
    return undefined;
  },
  // table.removeToken (crates/table/data-capabilities/remove-token): one
  // token taken off the map, `payload.token` on the instance `targetId` — or
  // the token itself as the target when `payload.token` is absent or null.
  // Its partition is tombstoned and the tables follow the aggregate's
  // relations: its entry leaves the order, whose places close up, and a turn
  // still on it is no one's — the Battlemap sends table.nextTurn first when
  // it holds the turn. An unknown token, or one of another instance than the
  // one named, is `not-found`. Ids are compared in any case and without the
  // blanks around them, as the server does.
  'table.removeToken': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const named = payload.token != null;
    const tokenId = named ? payload.token : targetId;
    if (typeof tokenId !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, tokenId) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => (instance == null || same(i.id, instance)) && i.state.tokens.some((t) => same(t.id, tokenId)));
    if (!holder) return ['not-found'];
    const id = holder.state.tokens.find((t) => same(t.id, tokenId)).id;
    holder.state.tokens = holder.state.tokens.filter((t) => t.id !== id);
    const combat = holder.state.combat;
    if (combat) {
      combat.order = (combat.order || []).filter((o) => o.id !== id);
      if (combat.current === id) combat.current = null;
    }
    return undefined;
  },
  // table.nextTurn (crates/table/data-capabilities/next-turn, Initiative@1,
  // « Suivant »): the open combat of the instance `targetId` — or
  // `payload.instance` — passes the turn to the next living participant after
  // the current one in its order, a hidden one like any other
  // (`hidden-keeps-rank`), a dead one or one off the map skipped
  // (`dead-derived`); the round goes up when the search wraps past the last
  // (`next-turn`). With no current turn, the first living one takes it and no
  // round is added. An instance with no open combat is refused by
  // `next-turn`, a combat with no one alive by `dead-derived`. Ids are
  // compared in any case and without the blanks around them, as the server
  // does.
  'table.nextTurn': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const id = targetId || payload?.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => same(i.id, id));
    const combat = holder?.state.combat;
    if (!combat) return ['next-turn'];
    const order = combat.order || [];
    const alive = order.map((p) => holder.state.tokens.some((t) => same(t.id, p.id) && !t.dead));
    const at = order.findIndex((p) => same(p.id, combat.current));
    const after = alive.findIndex((living, i) => living && i > at);
    const wrapped = at === -1 ? -1 : alive.findIndex((living, i) => living && i <= at);
    if (after === -1 && wrapped === -1) return ['dead-derived'];
    combat.current = order[after === -1 ? wrapped : after].id;
    if (after === -1) combat.round += 1;
    return undefined;
  },
  // table.startCombat (crates/table/data-capabilities/start-combat): the combat
  // of the instance `targetId` (or `payload.instance`), every token in it, a
  // hidden one like any other (`hidden-keeps-rank`). An NPC rolls
  // `payload.rolls[token]` or a d20, plus its DEX modifier (DEX 10 when its
  // sheet gives none); a PC rolls nothing and takes the initiative
  // `payload.initiative[character]` the DM entered (`npc-initiative-bounds`).
  // Ranked by initiative, then DEX score — an unknown one last —, then a random
  // draw (`initiative-tiebreak`); round 1 on rank 1. An unknown instance is
  // `not-found`; one whose combat is open starts none (`one-active-combat`);
  // one with no token, a token that is no PC nor NPC any more, a PC with no
  // initiative entered, or a roll out of 1 to 20 or of no NPC token is
  // refused by `npc-initiative-bounds`, as the server refuses them (422). Ids are
  // compared in any case and without the blanks around them, as the server
  // does.
  'table.startCombat': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const instance = instances.find((i) => same(i.id, id));
    if (!instance) return ['not-found'];
    if (instance.state.combat) return ['one-active-combat'];
    const tokens = instance.state.tokens;
    if (!tokens.length) return ['npc-initiative-bounds'];
    const figure = (figures, key) => Object.entries(figures || {}).find(([k]) => same(k, key))?.[1];
    if (Object.keys(payload.rolls || {}).some((k) => !tokens.some((t) => same(t.id, k) && t.kind === 'pnj'))) return ['npc-initiative-bounds'];
    const characters = await server.load('campaign.listCharacters');
    const seated = [];
    for (const t of tokens) {
      if (t.kind !== 'pj' && t.kind !== 'pnj') return ['npc-initiative-bounds'];
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
      .map(({ id: token, init, dexScore }) => ({ id: token, init, dex: dexScore === null ? 0 : Math.floor((dexScore - 10) / 2) }));
    instance.state.combat = { round: 1, current: order[0].id, order };
    return undefined;
  },
  // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative):
  // the open combat of the instance `targetId` (or `payload.instance`)
  // reordered as `payload.order` gives it, every participant once, named by
  // its token — the ranks 1 to n in that order, a hidden participant like any
  // other (`hidden-keeps-rank`); the turn and the round stay. An unknown
  // instance is `not-found`; one with no open combat, or an order that is no
  // list, omits a participant, repeats one or names one not in the combat, is
  // refused by `initiative-tiebreak`, as the server refuses them (422). Ids
  // are compared in any case and without the blanks around them, as the
  // server does.
  'table.reorderInitiative': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const id = targetId || payload.instance;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const instance = instances.find((i) => same(i.id, id));
    if (!instance) return ['not-found'];
    const combat = instance.state.combat;
    const held = (combat && combat.order) || [];
    const named = Array.isArray(payload.order) ? payload.order : [];
    if (!combat || !named.length || named.length !== held.length) return ['initiative-tiebreak'];
    const order = [];
    for (const n of named) {
      const at = held.findIndex((o) => same(o.id, n) && !order.includes(o));
      if (at < 0) return ['initiative-tiebreak'];
      order.push(held[at]);
    }
    combat.order = order;
    return undefined;
  },
  // table.removeParticipant (crates/table/data-capabilities/remove-participant):
  // one participant taken out of the open combat, `payload.participant` — named
  // by its token, as the carousel shows it — on the instance `targetId`, or the
  // participant itself as the target when `payload.participant` is absent or
  // null. Its entry is tombstoned and nothing else is written: its token stays
  // on the map, and the order closes up. The participant holding the turn is
  // refused by `next-turn`: the Battlemap sends table.nextTurn first. A
  // participant in no open combat, or in one of another instance than the one
  // named, is `not-found`. Ids are compared in any case and without the blanks
  // around them, as the server does.
  'table.removeParticipant': async (server, { payload, targetId }) => {
    const same = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? '').trim().toLowerCase();
    const named = payload?.participant != null;
    const participant = named ? payload.participant : targetId;
    if (typeof participant !== 'string') return ['not-found'];
    const instance = named && targetId && !same(targetId, participant) ? targetId : null;
    const fixture = await server.load('table.getBattlemapInstance');
    const instances = [fixture, ...(server.instances ? server.instances.values() : [])];
    const holder = instances.find((i) => (instance == null || same(i.id, instance)) && (i.state.combat?.order || []).some((o) => same(o.id, participant)));
    if (!holder) return ['not-found'];
    const combat = holder.state.combat;
    if (same(combat.current, participant)) return ['next-turn'];
    combat.order = combat.order.filter((o) => !same(o.id, participant));
    return undefined;
  },
};
