export const reads = {
  // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
  // is the scenes of session 4, the one in preparation; another session has none yet.
  'session.listScenes': async (server, { session }) =>
    session === 's4' ? await server.load('session.listScenes') : [],
};
