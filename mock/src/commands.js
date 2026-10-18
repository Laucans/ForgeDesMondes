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
