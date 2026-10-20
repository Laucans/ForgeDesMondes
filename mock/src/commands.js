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
  // session.createSession (planned, #157): the next session of the campaign, at
  // the position after the last, in preparation, with the PCs ticked present.
  // `present-only-pj` refuses a character that is not a PC of the fixtures.
  'session.createSession': async (server, { payload }) => {
    const sessions = await server.load('session.listSessions');
    const characters = await server.load('campaign.listCharacters');
    const present = Array.isArray(payload.present) ? payload.present : [];
    const pcs = new Set(characters.filter((c) => c.kind === 'pj').map((c) => c.id));
    if (present.some((id) => !pcs.has(id))) return ['present-only-pj'];
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
  // campaign.createCampaign (crates/campaign/data-capabilities/create-campaign):
  // a new campaign at the top of the home page, its master volume at full
  // (`master-volume-range`); a oneshot comes with its single session, in
  // preparation (`oneshot-single-session`), a campagne with none. A campaign
  // with no name is refused, as one of a kind there is not
  // (`oneshot-single-session`).
  'campaign.createCampaign': async (server, { payload }) => {
    const name = String(payload.name ?? '').trim();
    if (!name) return ['campaign-name-required'];
    if (!['campagne', 'oneshot'].includes(payload.type)) return ['oneshot-single-session'];
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
  // media.setMediaPlace (milestone #19): a medium's place set, or cleared with `place: null` — a medium
  // with no place stays valid. The place must be a lieu of the campaign (`media-place-is-lieu`).
  'media.setMediaPlace': async (server, { payload }) => {
    const library = await server.load('media.listMedia');
    const medium = library.battlemaps.concat(library.illustrations).find((m) => m.id === payload.media);
    if (!medium) return ['not-found'];
    const place = payload.place || null;
    if (place !== null && !(await server.load('campaign.listPlaces')).some((p) => p.id === place)) return ['media-place-is-lieu'];
    medium.place = place;
    return undefined;
  },
  // media.uploadMedia (milestone #19): a medium recorded, its place optional; a battlemap arrives with its
  // default grid (`grid-only-for-battlemap`, `hex-is-five-feet`).
  'media.uploadMedia': async (server, { payload }) => {
    const library = await server.load('media.listMedia');
    const place = payload.place || null;
    if (place !== null && !(await server.load('campaign.listPlaces')).some((p) => p.id === place)) return ['media-place-is-lieu'];
    const base = { id: `media-${server.uuid()}`, title: payload.title, file: payload.filePath, size: '2,2 Mo', uploadedAt: new Date().toISOString().slice(0, 10), place };
    if (payload.kind === 'battlemap') {
      library.battlemaps.push({ ...base, art: 'night', glyph: '⬡', width: payload.widthPx || 900, height: payload.heightPx || 600, grid: { size: 32, ox: 0, oy: 0, orient: 'pointy', color: 'text', opacity: 45 }, terrain: [] });
    } else library.illustrations.push({ ...base, art: 'dusk', glyph: String(payload.title).charAt(0).toUpperCase(), ratio: 1.5 });
    return undefined;
  },
  // session.addPlaceToSession (task #162): the place joins a scene — the one the payload names, else the
  // first — once, and its media follow: session-media-by-place, derived, never stored.
  'session.addPlaceToSession': async (server, { payload }) => {
    if (!(await server.load('campaign.listPlaces')).some((p) => p.id === payload.place)) return ['scene-links-same-campaign'];
    const scenes = await server.load('session.listScenes');
    const scene = scenes.find((sc) => sc.id === payload.scene) || scenes[0];
    if (!scene) return ['scene-links-same-campaign'];
    if (!scene.places.includes(payload.place)) scene.places.push(payload.place);
    const library = await server.load('media.listMedia');
    for (const m of library.battlemaps.concat(library.illustrations)) if (m.place === payload.place && !scene.media.includes(m.id)) scene.media.push(m.id);
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
