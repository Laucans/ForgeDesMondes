export const commands = {
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
};
