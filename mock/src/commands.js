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
};
