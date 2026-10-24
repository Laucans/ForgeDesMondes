export const commands = {
  // session.addPresentCharacter (crates/session/data-capabilities/add-present-character):
  // one more PC present at the session — `targetId`, or `payload.session` —
  // listed in its `present` from then on. `present-only-pj` refuses a
  // character that is not a PC of the fixtures, which hold one campaign; a
  // session they do not list is `not-found`. A PC present already stays
  // present once.
  'session.addPresentCharacter': async (server, { payload, targetId }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === (targetId || payload.session));
    if (!session) return ['not-found'];
    const characters = await server.load('campaign.listCharacters');
    if (!characters.some((c) => c.kind === 'pj' && c.id === payload.character)) return ['present-only-pj'];
    if (!session.present.includes(payload.character)) session.present.push(payload.character);
    return undefined;
  },
};
