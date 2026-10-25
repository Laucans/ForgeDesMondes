export const commands = {
  // campaign.importCampaign (crates/campaign/data-capabilities/import-campaign):
  // an archive always imports as a new campaign, at the top of the home page,
  // under a new id, whatever campaign it was made from and even when that one
  // still exists; the original is never touched (`import-creates-copy`). The
  // archive is the object `campaign.exportCampaign` wrote, or its text. One
  // that is none, of another format or of another version, is refused
  // `unprocessable`, and nothing is added.
  'campaign.importCampaign': async (server, { payload, targetId }) => {
    if (targetId) return ['unprocessable'];
    let archive = payload.archive;
    if (typeof archive === 'string') {
      try {
        archive = JSON.parse(archive);
      } catch {
        return ['unprocessable'];
      }
    }
    if (!archive || typeof archive !== 'object' || Array.isArray(archive)) return ['unprocessable'];
    if (archive.format !== 'campaign-archive' || archive.version !== 1) return ['unprocessable'];
    const campaign = archive.campaign;
    if (!campaign || typeof campaign !== 'object') return ['unprocessable'];
    const text = (field) => (typeof campaign[field] === 'string' ? campaign[field] : null);
    (await server.load('campaign.listCampaigns')).unshift({
      id: server.uuid(),
      name: text('name') ?? '',
      type: campaign.kind === 'oneshot' ? 'oneshot' : 'campagne',
      art: text('art'),
      glyph: text('glyph'),
      pitch: text('pitch'),
      sessions: Array.isArray(archive.sessions) ? archive.sessions.length : 0,
      lastPlayed: null,
      next: null,
    });
    return undefined;
  },
};
