export const reads = {
  // session.getLastBrief (crates/session/capabilities/get-last-brief): a session's last oral brief and what its
  // analysis found. The fixture is the brief of session 4; another session has none: the same shape, empty.
  'session.getLastBrief': async (server, { session }) =>
    session === 's4'
      ? await server.load('session.getLastBrief')
      : { session, lastRun: null, duration: null, transcript: [], result: { scenes: [], recognized: [], missing: [] } },
};
