export const commands = {
  // session.removePresentCharacter (crates/session/data-capabilities/remove-present-character):
  // a PC no longer present at the session — `targetId`, or `payload.session`
  // — taken out of its `present` (which `session.getSession` reads too), and
  // so no longer offered (`present-only-pj`); the character itself stays in
  // the fixtures. A PC who is not present changes nothing; a session they do
  // not list is `not-found`.
  'session.removePresentCharacter': async (server, { payload, targetId }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === (targetId || payload.session));
    if (!session) return ['not-found'];
    session.present = session.present.filter((pc) => pc !== payload.character);
    return undefined;
  },
};
