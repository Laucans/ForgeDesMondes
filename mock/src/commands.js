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

// SoundTag@1 as the fixtures know it: lower case, trimmed, no duplicate.
function soundTags(tags) {
  const list = Array.isArray(tags) ? tags : [];
  return [...new Set(list.map((t) => String(t).trim().toLowerCase()).filter(Boolean))];
}

export const COMMANDS = {
  // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
  // added to the library. Its tags are normalised as SoundTag@1 does (lower
  // case, no duplicate), its volume is a percentage that stays in [0, 100]
  // (`sound-volume-range`).
  'soundbox.addSound': async (server, { payload }) => {
    const volume = Number(payload.volume);
    if (!(volume >= 0 && volume <= 100)) return ['sound-volume-range'];
    const library = await server.load('soundbox.listSounds');
    library.sounds.unshift({ id: server.uuid(), name: String(payload.name || ''), tags: soundTags(payload.tags), duration: 2, volume, file: String(payload.file || ''), playing: false });
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
  // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
  // playback of the sound starts, whatever else plays (`simultaneous-playback`).
  'soundbox.playSound': async (server, { targetId }) => {
    const library = await server.load('soundbox.listSounds');
    if (!library.sounds.some((sound) => sound.id === targetId)) return ['not-found'];
    (server.playing = server.playing || new Set()).add(targetId);
    return undefined;
  },
  // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
  // running playback gets its stop (`stop-all`); with none, nothing changes.
  'soundbox.stopAll': async (server) => {
    (server.playing = server.playing || new Set()).clear();
    return undefined;
  },
  // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
  // one sound's own volume, a percent from 0 to 100 (`sound-volume-range`).
  'soundbox.setSoundVolume': async (server, { payload, targetId }) => {
    const sound = (await server.load('soundbox.listSounds')).sounds.find((candidate) => candidate.id === targetId);
    if (!sound) return ['not-found'];
    if (!(payload.volume >= 0 && payload.volume <= 100)) return ['sound-volume-range'];
    sound.volume = payload.volume;
    return undefined;
  },
  // soundbox.setMasterVolume (crates/soundbox/data-capabilities/set-master-volume):
  // the campaign's master volume, a percent from 0 to 100 (`master-volume-range`).
  'soundbox.setMasterVolume': async (server, { payload }) => {
    if (!(payload.volume >= 0 && payload.volume <= 100)) return ['master-volume-range'];
    (await server.load('soundbox.listSounds')).masterVolume = payload.volume;
    return undefined;
  },
};
