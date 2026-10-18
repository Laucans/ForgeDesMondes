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
};
