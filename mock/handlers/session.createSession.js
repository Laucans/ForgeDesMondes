export const commands = {
  // session.createSession (crates/session/data-capabilities/create-session):
  // the next session of the campaign, at the position after the last, in
  // preparation, with the PCs ticked present. `present-only-pj` refuses a
  // character that is not a PC of the fixtures; `oneshot-single-session` a
  // second session of a oneshot.
  'session.createSession': async (server, { payload }) => {
    const sessions = await server.load('session.listSessions');
    const characters = await server.load('campaign.listCharacters');
    const campaign = ((await server.load('campaign.listCampaigns')) || []).find((c) => c.id === payload.campaign);
    const present = Array.isArray(payload.present) ? payload.present : [];
    const pcs = new Set(characters.filter((c) => c.kind === 'pj').map((c) => c.id));
    if (present.some((id) => !pcs.has(id))) return ['present-only-pj'];
    if (campaign && campaign.type === 'oneshot' && (campaign.sessions ?? 0) > 0) return ['oneshot-single-session'];
    if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
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
};
